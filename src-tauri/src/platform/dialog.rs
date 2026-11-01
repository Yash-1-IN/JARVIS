//! The OS folder picker (Finder on macOS, Explorer on Windows, GTK on Linux).
//!
//! Called only from Rust: the dialog plugin is registered, but the webview has no
//! permission to call it (capabilities stay `core:default`). Picking a folder grants
//! nothing by itself; the path is only proposed and goes through the usual settings
//! validation before JARVIS may use it.

use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::DialogExt;

/// Opens the picker and waits for the answer without blocking a thread: the dialog
/// runs where the OS needs it (the main thread on macOS) and replies through a channel.
/// `None` when the user cancels.
pub async fn pick_folder<R: Runtime>(app: &AppHandle<R>, title: &str) -> Option<PathBuf> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app.dialog().file().set_title(title);
    // Attached to the main window, so it is a sheet on macOS and modal elsewhere.
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog.pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    rx.await.ok().flatten().and_then(|p| p.into_path().ok())
}

/// The picked folder as the text the settings store keeps. A folder whose name is not
/// valid Unicode cannot be stored (settings are JSON strings), so it is refused here
/// rather than mangled.
pub fn as_setting(path: &Path) -> Option<String> {
    path.to_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unicode_path_is_kept_exactly() {
        let p = Path::new("/Users/kim/Documents/Résumés 2026");
        assert_eq!(
            as_setting(p).as_deref(),
            Some("/Users/kim/Documents/Résumés 2026")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_non_unicode_path_is_refused_not_mangled() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let p = Path::new(OsStr::from_bytes(b"/tmp/bad\xffname"));
        assert_eq!(as_setting(p), None);
    }
}
