//! Path scope (spec §9.2 "scope escape"). A path is accepted only if, after resolving
//! symlinks and `..`, it lies inside an allowed root. Call `check_*` again immediately
//! before execution: a path can change between approval and use (symlink swap).

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
    /// Canonical roots (symlinks resolved).
    roots: Vec<PathBuf>,
    /// Windows and macOS file systems ignore case.
    case_insensitive: bool,
}

impl PathScope {
    /// Roots that no longer exist are dropped, so they allow nothing.
    pub fn new(roots: &[String], case_insensitive: bool) -> Self {
        let roots = roots
            .iter()
            .filter_map(|r| std::fs::canonicalize(r).ok())
            .filter(|r| r.is_dir())
            .collect();
        Self {
            roots,
            case_insensitive,
        }
    }

    /// For the running OS.
    pub fn for_this_os(roots: &[String]) -> Self {
        Self::new(roots, cfg!(any(windows, target_os = "macos")))
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
        let parent = p.parent().ok_or(PathError::Invalid)?;
        let canon_parent = std::fs::canonicalize(parent).map_err(|_| PathError::Missing)?;
        let canon_parent = self.inside(canon_parent)?;
        Ok(canon_parent.join(name))
    }

    fn inside(&self, canon: PathBuf) -> Result<PathBuf, PathError> {
        if self
            .roots
            .iter()
            .any(|r| within(r, &canon, self.case_insensitive))
        {
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

/// True if `path` is `root` or below it, compared component by component, so a sibling
/// such as `/data/root-evil` is not inside `/data/root`.
fn within(root: &Path, path: &Path, case_insensitive: bool) -> bool {
    if !case_insensitive {
        return path.starts_with(root);
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

    fn scope(root: &Path) -> PathScope {
        PathScope::new(&[root.to_string_lossy().into_owned()], false)
    }

    #[test]
    fn accepts_files_inside_and_resolves_dot_dot() {
        let root = dir("in");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/a.txt"), "x").unwrap();
        let s = scope(&root);
        let tricky = root.join("sub").join("..").join("sub").join("a.txt");
        assert_eq!(
            s.check_existing(&tricky.to_string_lossy()).unwrap(),
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
        assert_eq!(
            s.check_existing(&up.to_string_lossy()),
            Err(PathError::OutsideRoots)
        );
        // A sibling that merely starts with the root's name is not inside it.
        let evil = PathBuf::from(format!("{}-evil", root.display()));
        std::fs::create_dir_all(&evil).unwrap();
        assert_eq!(
            s.check_existing(&evil.to_string_lossy()),
            Err(PathError::OutsideRoots)
        );
        let elsewhere = s.check_existing("/etc/hosts");
        assert!(
            matches!(elsewhere, Err(PathError::OutsideRoots | PathError::Missing)),
            "{elsewhere:?}"
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
            s.check_existing(&via_link.to_string_lossy()),
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
        assert!(s.check_existing(&target.to_string_lossy()).is_ok());
        // Before execution it is replaced by a symlink to a file outside the root.
        std::fs::remove_file(&target).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), &target).unwrap();
        assert_eq!(
            s.check_existing(&target.to_string_lossy()),
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
            scope(&root).check_existing(&shouted.to_string_lossy()),
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
        assert_eq!(s.check_new(&ok.to_string_lossy()).unwrap(), ok);
        assert_eq!(
            s.check_new(&outside.join("x.txt").to_string_lossy()),
            Err(PathError::OutsideRoots)
        );
        assert_eq!(
            s.check_new(&root.join("missing-dir").join("x.txt").to_string_lossy()),
            Err(PathError::Missing)
        );
        let dotdot = format!("{}/Archive/..", root.display());
        assert_eq!(s.check_new(&dotdot), Err(PathError::Invalid));
    }

    #[test]
    fn missing_roots_allow_nothing() {
        let s = PathScope::new(&["/definitely/not/here".into()], false);
        assert_eq!(
            s.check_existing("/definitely/not/here/x"),
            Err(PathError::Missing)
        );
        let empty = PathScope::new(&[], false);
        assert_eq!(
            empty.check_existing(&std::env::temp_dir().to_string_lossy()),
            Err(PathError::OutsideRoots)
        );
    }
}
