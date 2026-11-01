//! File-system primitives the tools need but `std` does not offer (#22 review).
//!
//! `std::fs::rename` **replaces** an existing destination (`rename(2)` on Unix,
//! `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` on Windows), so checking "the destination does
//! not exist" first leaves a window where a file created meanwhile is silently clobbered.
//! `rename_no_replace` asks the OS to refuse instead, atomically.

use std::io;
use std::path::Path;

/// Moves `from` to `to` on the same volume, failing with `ErrorKind::AlreadyExists` if
/// `to` exists. Never overwrites. A folder is moved whole.
///
/// - Linux: `renameat2(RENAME_NOREPLACE)`. On file systems without it (`EINVAL`/`ENOSYS`,
///   e.g. some network mounts), a **file** falls back to `link` + `unlink` (`link` also
///   refuses an existing name); a folder is refused with `Unsupported` rather than risk it.
/// - macOS: `renamex_np(RENAME_EXCL)`.
/// - Windows: `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    imp::rename_no_replace(from, to)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod imp {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    fn c(p: &Path) -> io::Result<CString> {
        CString::new(p.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
    }

    pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
        let (f, t) = (c(from)?, c(to)?);
        // SAFETY: both pointers are valid NUL-terminated strings for the call's duration.
        let rc = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                f.as_ptr(),
                libc::AT_FDCWD,
                t.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if rc == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        match err.raw_os_error() {
            Some(libc::EINVAL) | Some(libc::ENOSYS) => link_then_unlink(from, to),
            _ => Err(err),
        }
    }

    /// Fallback for file systems without `RENAME_NOREPLACE`. Files only.
    fn link_then_unlink(from: &Path, to: &Path) -> io::Result<()> {
        if std::fs::symlink_metadata(from)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "this file system cannot move folders without risking an overwrite",
            ));
        }
        std::fs::hard_link(from, to)?; // EEXIST if `to` exists
        std::fs::remove_file(from)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    fn c(p: &Path) -> io::Result<CString> {
        CString::new(p.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
    }

    pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
        let (f, t) = (c(from)?, c(to)?);
        // SAFETY: both pointers are valid NUL-terminated strings for the call's duration.
        let rc = unsafe { libc::renamex_np(f.as_ptr(), t.as_ptr(), libc::RENAME_EXCL) };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    fn wide(p: &Path) -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
        let (f, t) = (wide(from), wide(to));
        // Flags 0: no MOVEFILE_REPLACE_EXISTING (so an existing `to` fails with
        // ERROR_ALREADY_EXISTS) and no MOVEFILE_COPY_ALLOWED (same volume only).
        // SAFETY: both buffers are valid NUL-terminated UTF-16 for the call's duration.
        let ok = unsafe { MoveFileExW(f.as_ptr(), t.as_ptr(), 0) };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    windows
)))]
mod imp {
    use std::io;
    use std::path::Path;

    pub fn rename_no_replace(_from: &Path, _to: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no atomic no-replace rename on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("jarvis-fs-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn moves_a_file_to_a_free_name() {
        let d = tmp();
        std::fs::write(d.join("a.txt"), "A").unwrap();
        rename_no_replace(&d.join("a.txt"), &d.join("b.txt")).unwrap();
        assert!(!d.join("a.txt").exists());
        assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap(), "A");
    }

    #[test]
    fn refuses_to_overwrite_and_leaves_both_files_intact() {
        let d = tmp();
        std::fs::write(d.join("a.txt"), "A").unwrap();
        std::fs::write(d.join("b.txt"), "B").unwrap();
        let e = rename_no_replace(&d.join("a.txt"), &d.join("b.txt")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists, "{e:?}");
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "A");
        assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap(), "B");
    }

    #[test]
    fn moves_a_folder_whole_but_never_onto_an_existing_one() {
        let d = tmp();
        std::fs::create_dir_all(d.join("src/inner")).unwrap();
        std::fs::write(d.join("src/inner/f.txt"), "x").unwrap();
        std::fs::create_dir_all(d.join("taken")).unwrap();
        let e = rename_no_replace(&d.join("src"), &d.join("taken")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists, "{e:?}");
        rename_no_replace(&d.join("src"), &d.join("dst")).unwrap();
        assert!(d.join("dst/inner/f.txt").is_file());
        assert!(!d.join("src").exists());
    }

    #[test]
    fn a_missing_source_is_an_error() {
        let d = tmp();
        let e = rename_no_replace(&d.join("nope"), &d.join("x")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
    }
}
