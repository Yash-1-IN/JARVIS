//! Path scope (spec §9.2 "scope escape"). A path is accepted only if, after resolving
//! symlinks and `..`, it lies inside an allowed root. Call `check_*` again immediately
//! before execution: a path can change between approval and use (symlink swap).
//!
//! Known limits (see docs/threat-model.md): there is still a short window between the last
//! check and the actual open or rename, which only `openat`/`O_NOFOLLOW` (Unix) or handle
//! based moves (Windows) can close; and a hard link inside a root to a file outside it
//! passes canonicalisation, because hard links cannot be detected portably.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("the path must be absolute")]
    NotAbsolute,
    #[error("network and device paths are not allowed")]
    DevicePath,
    #[error("the path is outside the allowed folders")]
    OutsideRoots,
    #[error("the path does not exist")]
    Missing,
    #[error("the path is not valid")]
    Invalid,
}

pub struct PathScope {
    /// Canonical roots (symlinks resolved), each with whether its file system ignores case.
    roots: Vec<(PathBuf, bool)>,
}

impl PathScope {
    /// Roots that no longer exist are dropped, so they allow nothing. Whether a root's file
    /// system ignores case is probed per root, not assumed from the OS: APFS and NTFS
    /// folders can be case-sensitive, and treating them as insensitive would let a
    /// differently-cased sibling through.
    pub fn new(roots: &[String]) -> Self {
        let roots = roots
            .iter()
            .filter_map(|r| std::fs::canonicalize(r).ok())
            .filter(|r| r.is_dir())
            .map(|r| {
                let ci = fs_ignores_case(&r);
                (r, ci)
            })
            .collect();
        Self { roots }
    }

    /// An existing file or folder inside the roots. Returns its canonical path.
    pub fn check_existing(&self, raw: &str) -> Result<PathBuf, PathError> {
        let p = pre_check(raw)?;
        let canon = std::fs::canonicalize(p).map_err(|_| PathError::Missing)?;
        self.inside(canon)
    }

    /// A path that does not exist yet (the destination of a move or rename): its parent
    /// must exist inside the roots and the last component must be a plain name.
    pub fn check_new(&self, raw: &str) -> Result<PathBuf, PathError> {
        let p = pre_check(raw)?;
        let name = p.file_name().ok_or(PathError::Invalid)?;
        if matches!(p.components().next_back(), Some(Component::ParentDir)) {
            return Err(PathError::Invalid);
        }
        if !valid_leaf_name(&name.to_string_lossy(), cfg!(windows)) {
            return Err(PathError::Invalid);
        }
        let parent = p.parent().ok_or(PathError::Invalid)?;
        let canon_parent = std::fs::canonicalize(parent).map_err(|_| PathError::Missing)?;
        let canon_parent = self.inside(canon_parent)?;
        Ok(canon_parent.join(name))
    }

    fn inside(&self, canon: PathBuf) -> Result<PathBuf, PathError> {
        if self.roots.iter().any(|(r, ci)| within(r, &canon, *ci)) {
            Ok(canon)
        } else {
            Err(PathError::OutsideRoots)
        }
    }
}

/// Rejects what must never reach the file system: NUL bytes, UNC and device paths, and
/// anything not absolute. String-level, so it behaves the same on every OS.
fn pre_check(raw: &str) -> Result<&Path, PathError> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains('\0') {
        return Err(PathError::Invalid);
    }
    // Two leading separators of either kind: \\server\share, //server/share, \\?\, \\.\
    let sep = |c: char| c == '/' || c == '\\';
    let mut c = raw.chars();
    if matches!((c.next(), c.next()), (Some(a), Some(b)) if sep(a) && sep(b)) {
        return Err(PathError::DevicePath);
    }
    let p = Path::new(raw);
    if !p.is_absolute() {
        return Err(PathError::NotAbsolute);
    }
    Ok(p)
}

/// Whether the file system holding `root` ignores case, found by asking it: the same name
/// with its case swapped resolves to the same folder. A name with no letters cannot tell,
/// so fall back to the platform default.
fn fs_ignores_case(root: &Path) -> bool {
    let default = cfg!(windows);
    let (Some(name), Some(parent)) = (root.file_name(), root.parent()) else {
        return default;
    };
    let name = name.to_string_lossy();
    let flipped: String = name
        .chars()
        .map(|c| {
            if c.is_lowercase() {
                c.to_uppercase().next().unwrap_or(c)
            } else if c.is_uppercase() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                c
            }
        })
        .collect();
    if flipped == name {
        return default;
    }
    std::fs::canonicalize(parent.join(flipped)).is_ok_and(|c| c == root)
}

/// A destination file name that means what it says on every OS: no `:` (an alternate data
/// stream on NTFS, a separator on classic macOS), no control characters, not `.`/`..`.
/// With `windows_rules` it also refuses reserved device names (`CON`, `NUL`, `COM1`...),
/// names Windows silently rewrites (trailing dot or space) and `<>"|?*`.
fn valid_leaf_name(name: &str, windows_rules: bool) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name
        .chars()
        .any(|c| c == ':' || c == '/' || c == '\\' || c.is_control())
    {
        return false;
    }
    if windows_rules {
        if name.ends_with('.') || name.ends_with(' ') {
            return false;
        }
        if name
            .chars()
            .any(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
        {
            return false;
        }
        let stem = name
            .split('.')
            .next()
            .unwrap_or(name)
            .trim_end()
            .to_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|p| {
                stem.strip_prefix(p)
                    .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit())
            });
        if reserved {
            return false;
        }
    }
    true
}

/// True if `path` is `root` or below it, compared component by component, so a sibling
/// such as `/data/root-evil` is not inside `/data/root`.
fn within(root: &Path, path: &Path, case_insensitive: bool) -> bool {
    if path.starts_with(root) {
        return true; // an exact match is always right
    }
    if !case_insensitive {
        return false;
    }
    let mut r = root.components();
    let mut p = path.components();
    loop {
        match (r.next(), p.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(a), Some(b)) => {
                let (a, b) = (
                    a.as_os_str().to_string_lossy(),
                    b.as_os_str().to_string_lossy(),
                );
                if a.to_lowercase() != b.to_lowercase() {
                    return false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jarvis-{name}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(d).unwrap()
    }

    /// The plain form of a path, as a tool would receive it. On Windows `canonicalize`
    /// returns `\\?\C:\...`, which `check_*` rightly refuses as a device path.
    fn plain(p: &Path) -> String {
        crate::config::display_path(p)
    }

    fn scope(root: &Path) -> PathScope {
        PathScope::new(&[root.to_string_lossy().into_owned()])
    }

    #[test]
    fn accepts_files_inside_and_resolves_dot_dot() {
        let root = dir("in");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/a.txt"), "x").unwrap();
        let s = scope(&root);
        let tricky = root.join("sub").join("..").join("sub").join("a.txt");
        assert_eq!(
            s.check_existing(&plain(&tricky)).unwrap(),
            root.join("sub/a.txt")
        );
    }

    #[test]
    fn rejects_dot_dot_escape_siblings_and_other_places() {
        let root = dir("root");
        let outside = dir("outside");
        std::fs::write(outside.join("secret.txt"), "x").unwrap();
        let s = scope(&root);
        let up = root
            .join("..")
            .join(outside.file_name().unwrap())
            .join("secret.txt");
        assert_eq!(s.check_existing(&plain(&up)), Err(PathError::OutsideRoots));
        // A sibling that merely starts with the root's name is not inside it.
        let evil = PathBuf::from(format!("{}-evil", root.display()));
        std::fs::create_dir_all(&evil).unwrap();
        assert_eq!(
            s.check_existing(&plain(&evil)),
            Err(PathError::OutsideRoots)
        );
        // An existing folder that is not the root (its parent) is outside it.
        let parent = root.parent().unwrap();
        assert_eq!(
            s.check_existing(&plain(parent)),
            Err(PathError::OutsideRoots)
        );
    }

    #[test]
    fn rejects_relative_unc_device_and_nul_paths_on_every_os() {
        let root = dir("shape");
        let s = scope(&root);
        for (bad, why) in [
            ("relative/file.txt", PathError::NotAbsolute),
            ("file.txt", PathError::NotAbsolute),
            (r"\\server\share\file", PathError::DevicePath),
            ("//server/share/file", PathError::DevicePath),
            (r"\\?\C:\Windows", PathError::DevicePath),
            (r"\\.\PhysicalDrive0", PathError::DevicePath),
            ("/ok/path\0hidden", PathError::Invalid),
            ("", PathError::Invalid),
            ("   ", PathError::Invalid),
        ] {
            assert_eq!(s.check_existing(bad), Err(why), "{bad:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_the_root_is_rejected() {
        let root = dir("link-root");
        let outside = dir("link-outside");
        std::fs::write(outside.join("secret.txt"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
        let s = scope(&root);
        let via_link = root.join("escape").join("secret.txt");
        assert_eq!(
            s.check_existing(&plain(&via_link)),
            Err(PathError::OutsideRoots)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_swapped_for_a_symlink_after_approval_fails_the_recheck() {
        let root = dir("swap-root");
        let outside = dir("swap-outside");
        let target = root.join("report.txt");
        std::fs::write(&target, "x").unwrap();
        std::fs::write(outside.join("secret.txt"), "x").unwrap();
        let s = scope(&root);
        // At approval time the file is fine.
        assert!(s.check_existing(&plain(&target)).is_ok());
        // Before execution it is replaced by a symlink to a file outside the root.
        std::fs::remove_file(&target).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), &target).unwrap();
        assert_eq!(
            s.check_existing(&plain(&target)),
            Err(PathError::OutsideRoots)
        );
    }

    #[test]
    fn case_variants_match_only_on_case_insensitive_systems() {
        let root = dir("Case");
        std::fs::write(root.join("a.txt"), "x").unwrap();
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        let upper = root.parent().unwrap().join(name.to_uppercase());
        let shouted = upper.join("a.txt");
        // The comparison itself, independent of what the file system does.
        assert!(within(&root, &shouted, true));
        assert!(!within(&root, &shouted, false));
        // On a case-sensitive file system the upper-case path does not exist at all.
        #[cfg(target_os = "linux")]
        assert_eq!(
            scope(&root).check_existing(&plain(&shouted)),
            Err(PathError::Missing)
        );
    }

    #[test]
    fn new_destinations_need_an_in_scope_parent_and_a_plain_name() {
        let root = dir("dest");
        let outside = dir("dest-outside");
        std::fs::create_dir_all(root.join("Archive")).unwrap();
        let s = scope(&root);
        let ok = root.join("Archive").join("moved.txt");
        assert_eq!(s.check_new(&plain(&ok)).unwrap(), ok);
        assert_eq!(
            s.check_new(&plain(&outside.join("x.txt"))),
            Err(PathError::OutsideRoots)
        );
        assert_eq!(
            s.check_new(&plain(&root.join("missing-dir").join("x.txt"))),
            Err(PathError::Missing)
        );
        let dotdot = format!("{}/Archive/..", plain(&root));
        assert_eq!(s.check_new(&dotdot), Err(PathError::Invalid));
    }

    #[test]
    fn missing_roots_allow_nothing() {
        let gone = std::env::temp_dir().join(format!("jarvis-gone-{}", uuid::Uuid::now_v7()));
        let s = PathScope::new(&[plain(&gone)]);
        assert_eq!(
            s.check_existing(&plain(&gone.join("x"))),
            Err(PathError::Missing)
        );
        let empty = PathScope::new(&[]);
        assert_eq!(
            empty.check_existing(&plain(&std::env::temp_dir())),
            Err(PathError::OutsideRoots)
        );
    }

    #[test]
    fn leaf_names_with_streams_devices_and_tricks_are_refused() {
        // On every OS: alternate streams, control characters, separators, dot names.
        for bad in ["report.txt:hidden", "a\u{7}b", "a/b", "a\\b", "", ".", ".."] {
            assert!(!valid_leaf_name(bad, false), "{bad:?}");
        }
        assert!(valid_leaf_name("report.txt", false));
        assert!(
            valid_leaf_name("CON", false),
            "reserved names matter only on Windows"
        );
        // Windows rules.
        for bad in [
            "CON",
            "con",
            "NUL.txt",
            "com1",
            "LPT9.log",
            "Aux ",
            "prn.tar.gz",
            "a.txt.",
            "a.txt ",
            "a<b",
            "a|b",
            "what?",
            "star*",
        ] {
            assert!(!valid_leaf_name(bad, true), "{bad:?}");
        }
        for ok in [
            "report.txt",
            "COM10",
            "console.log",
            "LPT",
            "a b.txt",
            "archive.2026.tar",
        ] {
            assert!(valid_leaf_name(ok, true), "{ok:?}");
        }
    }

    #[test]
    fn check_new_refuses_stream_names_on_this_os() {
        let root = dir("leaf");
        let s = scope(&root);
        let bad = root.join("report.txt:hidden");
        assert_eq!(s.check_new(&plain(&bad)), Err(PathError::Invalid));
    }

    #[test]
    fn case_sensitivity_is_probed_from_the_file_system_not_assumed() {
        let root = dir("MixedCase");
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        let flipped: String = name
            .chars()
            .map(|c| {
                if c.is_lowercase() {
                    c.to_ascii_uppercase()
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect();
        let sibling_resolves = root.parent().unwrap().join(&flipped).exists();
        assert_eq!(fs_ignores_case(&root), sibling_resolves);
    }

    #[test]
    fn a_case_sensitive_root_matches_only_exactly() {
        let root = PathBuf::from("/data/Docs");
        let sibling = PathBuf::from("/data/docs/file.txt");
        assert!(!within(&root, &sibling, false), "~/docs is not ~/Docs here");
        assert!(
            within(&root, &sibling, true),
            "but it is on a case-insensitive volume"
        );
        assert!(within(&root, &PathBuf::from("/data/Docs/file.txt"), false));
    }
}
