//! Installed apps for the `apps.launch` allow-list (contract #19). Spec §9.3 allows
//! **app IDs, never names**, so every entry carries a stable platform id:
//! - Linux: the `.desktop` file id (`org.mozilla.firefox.desktop`)
//! - macOS: the bundle id (`com.spotify.client`)
//! - Windows: the canonical `.exe` path a Start-menu shortcut points to
//!
//! The name is display only. Scans are cached because `settings_set` asks for the list
//! on every patch and walking Start-menu or Applications folders takes a moment.

use crate::ipc::types::AllowedApp;
use crate::services::InstalledApps;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(60);
/// Keeps a pathological system (or a hostile folder) from producing a huge picker.
const MAX_APPS: usize = 2_000;
/// Folder nesting walked below each root.
const MAX_DEPTH: usize = 3;
/// Display names are shown in the settings UI and on approval cards.
const MAX_NAME_CHARS: usize = 100;
/// `.desktop` files are tiny; anything larger is not one we trust.
pub(super) const MAX_DESKTOP_BYTES: u64 = 64 * 1024;
/// Real shortcuts are a few KB; larger `.lnk` files are not parsed at all.
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_LNK_BYTES: u64 = 64 * 1024;

type Scanner = Box<dyn Fn() -> Vec<AllowedApp> + Send + Sync>;

pub struct PlatformApps {
    scan: Scanner,
    cache: Mutex<Option<(Instant, Vec<AllowedApp>)>>,
}

impl PlatformApps {
    /// Scans the running OS.
    pub fn new() -> Self {
        Self::with_scanner(Box::new(scan_this_os))
    }

    pub fn with_scanner(scan: Scanner) -> Self {
        Self {
            scan,
            cache: Mutex::new(None),
        }
    }
}

impl Default for PlatformApps {
    fn default() -> Self {
        Self::new()
    }
}

impl InstalledApps for PlatformApps {
    fn list(&self) -> Vec<AllowedApp> {
        let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, apps)) = c.as_ref()
            && at.elapsed() < CACHE_TTL
        {
            return apps.clone();
        }
        let apps = finish((self.scan)());
        *c = Some((Instant::now(), apps.clone()));
        apps
    }
}

/// Clean names, drop duplicates (first id wins, so user-level entries scanned first
/// override system ones), sort for the picker, cap the size.
fn finish(raw: Vec<AllowedApp>) -> Vec<AllowedApp> {
    let mut out: Vec<AllowedApp> = Vec::new();
    for a in raw {
        let id = a.id.trim();
        let Some(name) = clean_name(&a.name) else {
            continue;
        };
        if id.is_empty() || id.chars().any(char::is_control) || out.iter().any(|o| o.id == id) {
            continue;
        }
        out.push(AllowedApp {
            id: id.to_owned(),
            name,
        });
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.truncate(MAX_APPS);
    out
}

fn clean_name(raw: &str) -> Option<String> {
    let name: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
/// Files under `root` (up to `MAX_DEPTH` folders deep) whose name ends with `ext`.
/// Symlinked folders are not followed, so a link loop cannot trap the scan.
pub(super) fn walk(root: &Path, ext: &str, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for e in entries.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_dir() && depth < MAX_DEPTH {
            walk(&p, ext, depth + 1, out);
        } else if (ft.is_file() || ft.is_symlink())
            && p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext))
        {
            out.push(p);
        }
    }
}

// ---------------------------------------------------------------- Linux (.desktop)

#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
/// The `Name` of an application entry that should appear in menus, or `None`.
/// Only the `[Desktop Entry]` group counts (actions have their own groups).
pub(super) fn desktop_entry_name(text: &str) -> Option<String> {
    let (mut in_entry, mut name, mut is_app) = (false, None, false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match (k.trim(), v.trim()) {
            ("Type", "Application") => is_app = true,
            ("NoDisplay", "true") | ("Hidden", "true") => return None,
            ("Name", v) if name.is_none() => name = Some(v.to_owned()),
            _ => {}
        }
    }
    if is_app { name } else { None }
}

#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
/// The desktop file id: the path below `applications/` with `/` turned into `-`.
pub(super) fn desktop_id(apps_dir: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(apps_dir).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Some(parts.join("-"))
}

#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
/// `$XDG_DATA_HOME` first (user overrides), then `$XDG_DATA_DIRS`, each `/applications`.
pub(super) fn xdg_application_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local/share")));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    data_home
        .into_iter()
        .chain(
            data_dirs
                .split(':')
                .filter(|d| !d.is_empty())
                .map(PathBuf::from),
        )
        .map(|d| d.join("applications"))
        .collect()
}

#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn scan_desktop_dirs(dirs: &[PathBuf]) -> Vec<AllowedApp> {
    let mut apps = Vec::new();
    for dir in dirs {
        let mut files = Vec::new();
        walk(dir, "desktop", 0, &mut files);
        for f in files {
            let small = std::fs::metadata(&f).is_ok_and(|m| m.len() <= MAX_DESKTOP_BYTES);
            let Some(text) = small.then(|| std::fs::read_to_string(&f).ok()).flatten() else {
                continue;
            };
            if let (Some(id), Some(name)) = (desktop_id(dir, &f), desktop_entry_name(&text)) {
                apps.push(AllowedApp { id, name });
            }
        }
    }
    apps
}

// ---------------------------------------------------------------- per-OS entry point

#[cfg(all(unix, not(target_os = "macos")))]
fn scan_this_os() -> Vec<AllowedApp> {
    scan_desktop_dirs(&xdg_application_dirs())
}

#[cfg(target_os = "macos")]
fn scan_this_os() -> Vec<AllowedApp> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut roots: Vec<PathBuf> = [
        "/Applications",
        "/Applications/Utilities",
        "/System/Applications",
        "/System/Applications/Utilities",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    roots.extend(home.map(|h| h.join("Applications")));
    let mut apps = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "app")
                && let Some(app) = macos_bundle(&p)
            {
                apps.push(app);
            }
        }
    }
    apps
}

/// Bundle id and display name from `Contents/Info.plist` (binary or XML).
#[cfg(target_os = "macos")]
fn macos_bundle(app: &Path) -> Option<AllowedApp> {
    let v = plist::Value::from_file(app.join("Contents/Info.plist")).ok()?;
    let d = v.as_dictionary()?;
    let s = |k: &str| d.get(k).and_then(|v| v.as_string()).map(str::to_owned);
    let id = s("CFBundleIdentifier").filter(|i| !i.is_empty())?;
    let name = s("CFBundleDisplayName")
        .or_else(|| s("CFBundleName"))
        .or_else(|| app.file_stem().map(|n| n.to_string_lossy().into_owned()))?;
    Some(AllowedApp { id, name })
}

#[cfg(windows)]
fn scan_this_os() -> Vec<AllowedApp> {
    let roots: Vec<PathBuf> = ["ProgramData", "APPDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|base| PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs"))
        .collect();
    let mut apps = Vec::new();
    for root in roots {
        let mut links = Vec::new();
        walk(&root, "lnk", 0, &mut links);
        apps.extend(links.iter().filter_map(|l| windows_shortcut(l)));
    }
    apps
}

/// A Start-menu shortcut to a local `.exe`. The id is the canonical exe path; the name
/// is the shortcut's own name. Uninstallers and network targets are skipped.
#[cfg(windows)]
fn windows_shortcut(link: &Path) -> Option<AllowedApp> {
    let name = link.file_stem()?.to_string_lossy().into_owned();
    if name.to_lowercase().contains("uninstall") {
        return None;
    }
    // Defence in depth against a hostile shortcut (any program can write the Start
    // menu): size and header first, then `lnk` under catch_unwind, because 0.6 panics
    // on some malformed links. The release profile unwinds so the catch works there.
    if std::fs::metadata(link).map_or(true, |m| m.len() > MAX_LNK_BYTES) {
        return None;
    }
    let mut head = [0u8; 20];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(link).ok()?;
        f.read_exact(&mut head).ok()?;
    }
    if !looks_like_shell_link(&head) {
        return None;
    }
    let target = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        lnk::ShellLink::open(link, lnk::encoding::WINDOWS_1252)
            .ok()
            .and_then(|l| l.link_target())
    }))
    .ok()??;
    if target.starts_with(r"\\") || !target.to_lowercase().ends_with(".exe") {
        return None;
    }
    let canon = std::fs::canonicalize(&target).ok()?;
    if !canon.is_file() {
        return None;
    }
    Some(AllowedApp {
        id: plain_windows_path(&canon),
        name,
    })
}

/// The fixed start of every shell link (MS-SHLLINK 2.1): header size 0x4C, then
/// CLSID 00021401-0000-0000-C000-000000000046 in little-endian layout. Checked before
/// the parser sees the file, so most junk never reaches `lnk` at all.
#[cfg_attr(not(windows), allow(dead_code))]
fn looks_like_shell_link(head: &[u8]) -> bool {
    const MAGIC: [u8; 20] = [
        0x4C, 0x00, 0x00, 0x00, 0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x46,
    ];
    head.len() >= MAGIC.len() && head[..MAGIC.len()] == MAGIC
}

/// `canonicalize` gives `\\?\C:\...`; ids use the plain drive form. Other `\\?\` forms
/// (UNC) never get here: network targets are skipped above.
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn plain_windows_path(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.len() > 2 && rest.as_bytes()[1] == b':' => rest.to_owned(),
        _ => s,
    }
}

#[cfg(test)]
mod tests;
