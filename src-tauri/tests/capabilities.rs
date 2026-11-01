//! Guard (CLAUDE.md security rules, #51 review): the webview's only permission is
//! `core:default`. Plugins such as the dialog plugin are used from Rust only; if a
//! capability file ever grants anything more (fs, shell, http, sql, opener, dialog...),
//! this test fails and the change needs an explicit decision.

use std::fs;
use std::path::Path;

#[test]
fn the_webview_has_only_core_default() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
    let mut files = 0;
    for entry in fs::read_dir(&dir).expect("capabilities folder") {
        let path = entry.expect("dir entry").path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        // Tauri also reads TOML capability files; nothing but JSON is expected here.
        assert_eq!(ext, "json", "unexpected capability file {}", path.display());
        files += 1;
        let text = fs::read_to_string(&path).expect("read capability");
        let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let permissions = json["permissions"]
            .as_array()
            .unwrap_or_else(|| panic!("{} has no permissions list", path.display()));
        let granted: Vec<String> = permissions
            .iter()
            .map(|p| match p {
                // An object grants a permission with a scope; name it by its identifier.
                serde_json::Value::Object(o) => o["identifier"].to_string(),
                other => other.as_str().unwrap_or_default().to_owned(),
            })
            .collect();
        assert_eq!(
            granted,
            vec!["core:default".to_owned()],
            "{} grants more than core:default",
            path.display()
        );
    }
    assert!(files > 0, "no capability file found");
}
