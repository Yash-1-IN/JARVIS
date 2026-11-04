//! Opens an address in the person's default browser, from Rust (no webview permission, no
//! `opener` plugin). Only `https://` addresses are opened, and the address is passed as one
//! argument to the system's opener, never through a shell.

use std::process::{Command, Stdio};

/// True if the system's opener started. Anything that is not `https://` is refused.
pub fn open_https(url: &str) -> bool {
    if !allowed(url) {
        return false;
    }
    let mut cmd = opener(url);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match cmd.status() {
        Ok(s) => s.success(),
        Err(e) => {
            tracing::warn!(error = %e, "could not open the browser");
            false
        }
    }
}

fn allowed(url: &str) -> bool {
    url.starts_with("https://") && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

#[cfg(target_os = "macos")]
fn opener(url: &str) -> Command {
    let mut c = Command::new("/usr/bin/open");
    c.arg(url);
    c
}

#[cfg(target_os = "windows")]
fn opener(url: &str) -> Command {
    // rundll32 takes the address as one argument; `cmd /c start` would let `&` end it. It is
    // run by its full path under the Windows folder, so a program of that name in the current
    // folder or on the path is never run instead.
    let exe = match std::env::var_os("SystemRoot") {
        Some(root) => std::path::Path::new(&root)
            .join("System32")
            .join("rundll32.exe"),
        None => "rundll32.exe".into(),
    };
    let mut c = Command::new(exe);
    c.arg("url.dll,FileProtocolHandler").arg(url);
    c
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn opener(url: &str) -> Command {
    let mut c = Command::new("xdg-open");
    c.arg(url);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_https_addresses_are_opened() {
        assert!(allowed(
            "https://accounts.spotify.com/authorize?client_id=x&scope=a%20b"
        ));
        for bad in [
            "http://accounts.spotify.com/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://x.com/ y",
            "https://x.com/\nopen -a Calculator",
            "",
        ] {
            assert!(!allowed(bad), "{bad:?}");
            assert!(!open_https(bad), "{bad:?}");
        }
    }
}
