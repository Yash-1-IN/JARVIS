//! Starting an allowed app (`apps.launch`, spec §9.4): by its stable id, with fixed
//! arguments and never through a shell. Whether the id is on the user's allow-list is the
//! tool's and gateway's job; this module only refuses ids that are malformed or no longer
//! resolve to an installed app, and starts nothing else.
//!
//! - Windows: the id is a canonical `.exe` path; it is started directly, no arguments.
//! - macOS: the id is a bundle id; `/usr/bin/open -b <id>`.
//! - Linux: the id is a `.desktop` file id; its file is found the same way the app list
//!   finds it, re-checked, and started with `gio launch <file>`.

use std::io;

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.to_owned())
}

/// Starts the app and returns once it has been handed to the OS (not when it exits).
/// Blocking for a moment on macOS and Linux; call from `spawn_blocking`.
pub fn launch(id: &str) -> io::Result<()> {
    imp::launch(id)
}

/// macOS bundle ids: reverse-DNS letters, digits, `.` and `-` (no spaces, no options).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn valid_bundle_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 255
        && !id.starts_with(['-', '.'])
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// Linux desktop file ids, e.g. `org.gnome.Calculator.desktop` or `kde4-kate.desktop`.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn valid_desktop_id(id: &str) -> bool {
    id.len() <= 255
        && id.len() > ".desktop".len()
        && id.ends_with(".desktop")
        && !id.starts_with(['-', '.'])
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Windows ids are plain absolute `.exe` paths (`C:\...\app.exe`), never UNC or device
/// paths, and never anything with control characters or quotes.
#[cfg_attr(not(windows), allow(dead_code))]
fn valid_exe_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() > 7
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && b[2] == b'\\'
        && id.to_ascii_lowercase().ends_with(".exe")
        && !id.chars().any(|c| c.is_control() || c == '"')
}

#[cfg(windows)]
mod imp {
    use super::{invalid, valid_exe_id};
    use std::io;
    use std::path::Path;
    use std::process::{Command, Stdio};

    pub fn launch(id: &str) -> io::Result<()> {
        if !valid_exe_id(id) {
            return Err(invalid("not an app id"));
        }
        let exe = Path::new(id);
        // Must still be the same, existing file: canonicalise and compare.
        let canon = std::fs::canonicalize(exe)?;
        let plain = super::super::apps::plain_windows_path(&canon);
        if !plain.eq_ignore_ascii_case(id) || !canon.is_file() {
            return Err(invalid("the app is no longer installed there"));
        }
        let dir = exe.parent().ok_or_else(|| invalid("not an app id"))?;
        Command::new(exe)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{invalid, valid_bundle_id};
    use std::io;
    use std::process::{Command, Stdio};

    pub fn launch(id: &str) -> io::Result<()> {
        if !valid_bundle_id(id) {
            return Err(invalid("not an app id"));
        }
        // `open -b` resolves the bundle id through Launch Services; a non-zero exit means
        // no such app. Absolute path, so PATH cannot substitute another `open`.
        let st = Command::new("/usr/bin/open")
            .args(["-b", id])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if st.success() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no app with that id",
            ))
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::{invalid, valid_desktop_id};
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    pub fn launch(id: &str) -> io::Result<()> {
        if !valid_desktop_id(id) {
            return Err(invalid("not an app id"));
        }
        let file = resolve(&super::super::apps::xdg_application_dirs(), id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no app with that id"))?;
        let gio = ["/usr/bin/gio", "/bin/gio"]
            .iter()
            .map(Path::new)
            .find(|p| p.is_file())
            .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "gio is not installed"))?;
        let st = Command::new(gio)
            .arg("launch")
            .arg(&file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if st.success() {
            Ok(())
        } else {
            Err(io::Error::other("the app could not be started"))
        }
    }

    /// The first `.desktop` file with this id in XDG precedence order that is still a
    /// visible application (the same rules the app list uses).
    pub(super) fn resolve(dirs: &[PathBuf], id: &str) -> Option<PathBuf> {
        use super::super::apps::{MAX_DESKTOP_BYTES, desktop_entry_name, desktop_id, walk};
        for dir in dirs {
            let mut files = Vec::new();
            walk(dir, "desktop", 0, &mut files);
            for f in files {
                if desktop_id(dir, &f).as_deref() != Some(id) {
                    continue;
                }
                let small = std::fs::metadata(&f).is_ok_and(|m| m.len() <= MAX_DESKTOP_BYTES);
                let text = small.then(|| std::fs::read_to_string(&f).ok()).flatten()?;
                // First match wins, as in the list: a hidden user entry hides the system one.
                return desktop_entry_name(&text).map(|_| f);
            }
        }
        None
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use std::io;
    pub fn launch(_id: &str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "not supported on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_ids() {
        for ok in [
            "com.spotify.client",
            "com.apple.Safari",
            "org.mozilla.firefox-esr",
        ] {
            assert!(valid_bundle_id(ok), "{ok}");
        }
        for bad in [
            "",
            "-b",
            ".hidden",
            "com.app;rm",
            "com app",
            "a/b",
            "com.app\n",
        ] {
            assert!(!valid_bundle_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn desktop_ids() {
        for ok in [
            "org.gnome.Calculator.desktop",
            "kde4-kate.desktop",
            "my_app.desktop",
        ] {
            assert!(valid_desktop_id(ok), "{ok}");
        }
        for bad in [
            ".desktop",
            "app",
            "-x.desktop",
            "../evil.desktop",
            "a/b.desktop",
            "a b.desktop",
            "a;b.desktop",
        ] {
            assert!(!valid_desktop_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn exe_ids() {
        assert!(valid_exe_id(r"C:\Program Files\App\app.exe"));
        assert!(valid_exe_id(r"d:\tools\X.EXE"));
        for bad in [
            r"\\server\share\app.exe",
            r"\\?\C:\app.exe",
            r"C:\app.bat",
            r"app.exe",
            r#"C:\a"b.exe"#,
            "C:\\a\nb.exe",
            r"C:/x.exe",
        ] {
            assert!(!valid_exe_id(bad), "{bad:?}");
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_resolve_follows_precedence_and_visibility() {
        use std::path::PathBuf;
        let tmp = |n: &str| {
            let d =
                std::env::temp_dir().join(format!("jarvis-launch-{n}-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&d).unwrap();
            d
        };
        let (user, system) = (tmp("user"), tmp("system"));
        let entry = |n: &str| format!("[Desktop Entry]\nType=Application\nName={n}\nExec=true\n");
        std::fs::write(system.join("music.desktop"), entry("Music")).unwrap();
        std::fs::create_dir_all(system.join("kde4")).unwrap();
        std::fs::write(system.join("kde4/kate.desktop"), entry("Kate")).unwrap();
        std::fs::write(user.join("hidden.desktop"), entry("H") + "Hidden=true\n").unwrap();
        std::fs::write(system.join("hidden.desktop"), entry("H")).unwrap();
        let dirs: Vec<PathBuf> = vec![user.clone(), system.clone()];

        assert_eq!(
            imp::resolve(&dirs, "music.desktop"),
            Some(system.join("music.desktop"))
        );
        assert_eq!(
            imp::resolve(&dirs, "kde4-kate.desktop"),
            Some(system.join("kde4/kate.desktop"))
        );
        assert_eq!(
            imp::resolve(&dirs, "hidden.desktop"),
            None,
            "user entry hides it"
        );
        assert_eq!(imp::resolve(&dirs, "missing.desktop"), None);
    }

    #[test]
    fn malformed_ids_are_refused_before_anything_starts() {
        for bad in ["", "-a", "; rm -rf ~", "../../bin/sh", "C:\\x.bat"] {
            assert_eq!(
                launch(bad).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "{bad:?}"
            );
        }
    }
}
