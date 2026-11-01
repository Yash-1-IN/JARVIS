use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn app(id: &str, name: &str) -> AllowedApp {
    AllowedApp {
        id: id.into(),
        name: name.into(),
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jarvis-apps-{name}-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn desktop_entry_needs_application_type_and_a_name() {
    let ok =
        "[Desktop Entry]\nType=Application\nName=Firefox\nName[de]=Feuerfuchs\nExec=firefox %u\n";
    assert_eq!(desktop_entry_name(ok).as_deref(), Some("Firefox"));
    for bad in [
        "[Desktop Entry]\nType=Link\nName=Docs\n",
        "[Desktop Entry]\nType=Application\nName=Hidden\nNoDisplay=true\n",
        "[Desktop Entry]\nType=Application\nName=Gone\nHidden=true\n",
        "[Desktop Entry]\nType=Application\n",
        "Type=Application\nName=No group\n",
    ] {
        assert_eq!(desktop_entry_name(bad), None, "{bad:?}");
    }
    // Keys in action groups do not leak into the entry.
    let actions = "[Desktop Entry]\nType=Application\nName=Editor\n\n[Desktop Action new]\nName=New Window\nNoDisplay=true\n";
    assert_eq!(desktop_entry_name(actions).as_deref(), Some("Editor"));
}

#[test]
fn desktop_id_follows_the_xdg_rule() {
    let base = Path::new("/usr/share/applications");
    assert_eq!(
        desktop_id(base, &base.join("org.gnome.Calculator.desktop")).as_deref(),
        Some("org.gnome.Calculator.desktop")
    );
    assert_eq!(
        desktop_id(base, &base.join("kde4/kate.desktop")).as_deref(),
        Some("kde4-kate.desktop")
    );
    assert_eq!(desktop_id(base, Path::new("/elsewhere/x.desktop")), None);
}

#[test]
fn scans_nested_dirs_and_user_entries_override_system_ones() {
    let user = tmp("user");
    let system = tmp("system");
    std::fs::create_dir_all(system.join("sub")).unwrap();
    let entry = |n: &str| format!("[Desktop Entry]\nType=Application\nName={n}\n");
    std::fs::write(user.join("music.desktop"), entry("My Music")).unwrap();
    std::fs::write(system.join("music.desktop"), entry("Music")).unwrap();
    std::fs::write(system.join("sub/notes.desktop"), entry("Notes")).unwrap();
    std::fs::write(
        system.join("hidden.desktop"),
        entry("X") + "NoDisplay=true\n",
    )
    .unwrap();
    std::fs::write(system.join("readme.txt"), "not an entry").unwrap();
    std::fs::write(system.join("huge.desktop"), "x".repeat(70_000)).unwrap();

    let apps = finish(scan_desktop_dirs(&[user, system]));
    assert_eq!(
        apps,
        vec![
            app("music.desktop", "My Music"),
            app("sub-notes.desktop", "Notes")
        ]
    );
}

#[test]
fn finish_cleans_dedupes_sorts_and_caps() {
    let long = "N".repeat(500);
    let out = finish(vec![
        app("b", "zebra"),
        app("a", "Apple\u{7}"),
        app("b", "duplicate id"),
        app("", "no id"),
        app("c", "   "),
        app("d\nx", "control in id"),
        app("e", &long),
    ]);
    let ids: Vec<&str> = out.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["a", "e", "b"]);
    assert_eq!(out[0].name, "Apple");
    assert_eq!(out[1].name.chars().count(), MAX_NAME_CHARS);

    let many: Vec<AllowedApp> = (0..MAX_APPS + 10)
        .map(|i| app(&format!("id{i}"), &format!("n{i}")))
        .collect();
    assert_eq!(finish(many).len(), MAX_APPS);
}

#[test]
fn list_is_cached() {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let apps = PlatformApps::with_scanner(Box::new(move || {
        c.fetch_add(1, Ordering::SeqCst);
        vec![app("x", "X")]
    }));
    assert_eq!(apps.list(), vec![app("x", "X")]);
    assert_eq!(apps.list().len(), 1);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "second call served from cache"
    );
}

#[test]
fn plain_windows_path_strips_only_drive_prefixes() {
    assert_eq!(
        plain_windows_path(Path::new(r"\\?\C:\Program Files\App\app.exe")),
        r"C:\Program Files\App\app.exe"
    );
    assert_eq!(
        plain_windows_path(Path::new(r"\\?\UNC\server\share\app.exe")),
        r"\\?\UNC\server\share\app.exe"
    );
}

#[test]
fn this_os_scan_never_panics() {
    // CI machines have few apps; the point is that the real scan runs cleanly.
    let n = PlatformApps::new().list().len();
    assert!(n <= MAX_APPS);
}
