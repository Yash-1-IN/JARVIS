//! Settings persisted as JSON in the app config directory (spec §8.2 `settings`, versioned).
//! The API key is never stored here; it lives in the keychain vault.

use crate::intelligence::pricing::{DEFAULT_MODEL, PriceTable};
use crate::ipc::types::{AllowedApp, SettingsPatch};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub const SCHEMA_VERSION: u32 = 1;
/// Sanity limit for a spend cap: $100,000 in micro-dollars.
pub const MAX_CAP_MICRO_USD: u64 = 100_000_000_000;
pub const MAX_ALLOWED_FOLDERS: usize = 50;
pub const MAX_ALLOWED_APPS: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSettings {
    pub schema_version: u32,
    pub model: String,
    pub daily_cap_micro_usd: Option<u64>,
    pub monthly_cap_micro_usd: Option<u64>,
    /// Canonical absolute paths the file tools may touch. Older files lack this field.
    #[serde(default)]
    pub allowed_folders: Vec<String>,
    /// Apps `apps.launch` may start, by stable id (the name is display only).
    #[serde(default)]
    pub allowed_apps: Vec<AllowedApp>,
    #[serde(default)]
    pub auto_allow_medium: bool,
}

impl Default for StoredSettings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            model: DEFAULT_MODEL.into(),
            daily_cap_micro_usd: None,
            monthly_cap_micro_usd: None,
            allowed_folders: vec![],
            allowed_apps: vec![],
            auto_allow_medium: false,
        }
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("`{0}` is not a supported model")]
    UnknownModel(String),
    #[error("a spend cap may not exceed $100,000")]
    CapTooLarge,
    #[error("`{0}` is not an existing folder given as an absolute path")]
    BadFolder(String),
    #[error("`{0}` is not an installed app")]
    UnknownApp(String),
    #[error("too many entries in an allow-list")]
    TooMany,
    #[error("settings could not be saved")]
    Save,
}

pub struct SettingsStore {
    path: PathBuf,
    inner: Mutex<StoredSettings>,
}

impl SettingsStore {
    /// Loads the file. A missing file gives the defaults. A corrupt file, a newer schema
    /// or an unpriced model is never fatal: the old file is kept as `settings.json.bak`
    /// and the affected values fall back to the defaults.
    pub fn load(path: PathBuf, prices: &PriceTable) -> Self {
        let mut s = match std::fs::read_to_string(&path) {
            Err(_) => StoredSettings::default(),
            Ok(text) => match serde_json::from_str::<StoredSettings>(&text) {
                Ok(v) if v.schema_version <= SCHEMA_VERSION => v,
                _ => {
                    tracing::warn!(
                        "settings file unreadable or from a newer version; using defaults"
                    );
                    let _ = std::fs::write(path.with_extension("json.bak"), text);
                    StoredSettings::default()
                }
            },
        };
        if prices.price(&s.model).is_err() {
            tracing::warn!(model = %s.model, "saved model has no price; using the default");
            s.model = DEFAULT_MODEL.into();
        }
        s.schema_version = SCHEMA_VERSION;
        Self {
            path,
            inner: Mutex::new(s),
        }
    }

    fn lock(&self) -> MutexGuard<'_, StoredSettings> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self) -> StoredSettings {
        self.lock().clone()
    }

    /// Validates and applies a patch, then saves. On any error nothing changes.
    /// Like `apply_with` when no apps are known: any non-empty app list is rejected.
    #[cfg(test)]
    pub fn apply(
        &self,
        patch: &SettingsPatch,
        prices: &PriceTable,
    ) -> Result<StoredSettings, SettingsError> {
        self.apply_with(patch, prices, &[])
    }

    /// Validates and applies a patch, then saves. `installed` is what `apps_installed`
    /// returns now: only those ids can be allowed. On any error nothing changes.
    pub fn apply_with(
        &self,
        patch: &SettingsPatch,
        prices: &PriceTable,
        installed: &[AllowedApp],
    ) -> Result<StoredSettings, SettingsError> {
        let mut next = self.get();
        if let Some(m) = &patch.model {
            prices
                .price(m)
                .map_err(|_| SettingsError::UnknownModel(m.clone()))?;
            next.model = m.clone();
        }
        if let Some(c) = patch.daily_cap_micro_usd {
            next.daily_cap_micro_usd = c;
        }
        if let Some(c) = patch.monthly_cap_micro_usd {
            next.monthly_cap_micro_usd = c;
        }
        if let Some(folders) = &patch.allowed_folders {
            next.allowed_folders = canonical_folders(folders)?;
        }
        if let Some(apps) = &patch.allowed_apps {
            next.allowed_apps = resolve_apps(apps, installed)?;
        }
        if let Some(on) = patch.auto_allow_medium {
            next.auto_allow_medium = on;
        }
        if [next.daily_cap_micro_usd, next.monthly_cap_micro_usd]
            .into_iter()
            .flatten()
            .any(|c| c > MAX_CAP_MICRO_USD)
        {
            return Err(SettingsError::CapTooLarge);
        }
        save_atomically(&self.path, &next).map_err(|e| {
            tracing::warn!(error = %e, "saving settings failed");
            SettingsError::Save
        })?;
        *self.lock() = next.clone();
        Ok(next)
    }
}

/// Each folder must be an absolute path to an existing directory. Stored canonical
/// (symlinks and `..` resolved), so later checks compare canonical with canonical.
fn canonical_folders(input: &[String]) -> Result<Vec<String>, SettingsError> {
    if input.len() > MAX_ALLOWED_FOLDERS {
        return Err(SettingsError::TooMany);
    }
    let mut out: Vec<String> = Vec::new();
    for raw in input {
        let p = Path::new(raw.trim());
        let bad = || SettingsError::BadFolder(raw.clone());
        if !p.is_absolute() {
            return Err(bad());
        }
        let canon = std::fs::canonicalize(p).map_err(|_| bad())?;
        if !canon.is_dir() {
            return Err(bad());
        }
        let shown = display_path(&canon);
        if !out.contains(&shown) {
            out.push(shown);
        }
    }
    Ok(out)
}

/// Windows returns `\\?\C:\...` from `canonicalize`; show the plain drive path.
/// UNC and other device paths (`\\?\UNC\...`) are left alone for the gateway to reject.
fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.len() > 2 && rest.as_bytes()[1] == b':' => rest.to_owned(),
        _ => s,
    }
}

/// Keeps only ids that are installed, and takes the display name from the installed list
/// so a patch cannot rename an app or smuggle in a spoofed name (spec §9.3).
fn resolve_apps(
    input: &[AllowedApp],
    installed: &[AllowedApp],
) -> Result<Vec<AllowedApp>, SettingsError> {
    if input.len() > MAX_ALLOWED_APPS {
        return Err(SettingsError::TooMany);
    }
    let mut out: Vec<AllowedApp> = Vec::new();
    for want in input {
        let app = installed
            .iter()
            .find(|a| a.id == want.id)
            .ok_or_else(|| SettingsError::UnknownApp(want.id.clone()))?;
        if !out.iter().any(|a| a.id == app.id) {
            out.push(app.clone());
        }
    }
    Ok(out)
}

/// Writes to a temporary file and renames it, so a crash never leaves half a file.
fn save_atomically(path: &Path, s: &StoredSettings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jarvis-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    fn app(id: &str, name: &str) -> AllowedApp {
        AllowedApp {
            id: id.into(),
            name: name.into(),
        }
    }

    fn installed() -> Vec<AllowedApp> {
        vec![
            app("com.spotify.client", "Spotify"),
            app("calc", "Calculator"),
        ]
    }

    fn prices() -> PriceTable {
        PriceTable::embedded().unwrap()
    }

    fn patch(json: &str) -> SettingsPatch {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn missing_file_gives_defaults() {
        let s = SettingsStore::load(tmp_path(), &prices());
        assert_eq!(s.get(), StoredSettings::default());
    }

    #[test]
    fn patch_applies_saves_and_reloads() {
        let path = tmp_path();
        let s = SettingsStore::load(path.clone(), &prices());
        let out = s
            .apply(
                &patch(r#"{"model":"claude-sonnet-5-5","dailyCapMicroUsd":5000000}"#),
                &prices(),
            )
            .unwrap();
        assert_eq!(out.model, "claude-sonnet-5-5");
        assert_eq!(out.daily_cap_micro_usd, Some(5_000_000));
        let again = SettingsStore::load(path, &prices());
        assert_eq!(again.get(), out);
    }

    #[test]
    fn omitted_cap_is_unchanged_and_null_clears_it() {
        let s = SettingsStore::load(tmp_path(), &prices());
        s.apply(
            &patch(r#"{"dailyCapMicroUsd":1000,"monthlyCapMicroUsd":9000}"#),
            &prices(),
        )
        .unwrap();
        let out = s
            .apply(&patch(r#"{"monthlyCapMicroUsd":null}"#), &prices())
            .unwrap();
        assert_eq!(
            (out.daily_cap_micro_usd, out.monthly_cap_micro_usd),
            (Some(1000), None)
        );
    }

    #[test]
    fn unknown_model_and_huge_caps_are_rejected_and_change_nothing() {
        let s = SettingsStore::load(tmp_path(), &prices());
        let before = s.get();
        assert_eq!(
            s.apply(
                &patch(r#"{"model":"gpt-4","dailyCapMicroUsd":1}"#),
                &prices()
            ),
            Err(SettingsError::UnknownModel("gpt-4".into()))
        );
        assert_eq!(
            s.apply(
                &patch(&format!(
                    r#"{{"monthlyCapMicroUsd":{}}}"#,
                    MAX_CAP_MICRO_USD + 1
                )),
                &prices()
            ),
            Err(SettingsError::CapTooLarge)
        );
        assert_eq!(s.get(), before);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_replaced_by_defaults() {
        let path = tmp_path();
        std::fs::write(&path, "{ not json").unwrap();
        let s = SettingsStore::load(path.clone(), &prices());
        assert_eq!(s.get(), StoredSettings::default());
        assert_eq!(
            std::fs::read_to_string(path.with_extension("json.bak")).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn newer_schema_and_unpriced_model_fall_back() {
        let path = tmp_path();
        std::fs::write(&path, r#"{"schemaVersion":99,"model":"claude-sonnet-5-5","dailyCapMicroUsd":5,"monthlyCapMicroUsd":null}"#).unwrap();
        assert_eq!(
            SettingsStore::load(path, &prices()).get(),
            StoredSettings::default()
        );
        let path = tmp_path();
        std::fs::write(&path, r#"{"schemaVersion":1,"model":"retired-model","dailyCapMicroUsd":7,"monthlyCapMicroUsd":null}"#).unwrap();
        let s = SettingsStore::load(path, &prices()).get();
        assert_eq!(
            (s.model.as_str(), s.daily_cap_micro_usd),
            (DEFAULT_MODEL, Some(7))
        );
    }

    #[test]
    fn misspelt_patch_field_is_an_error() {
        assert!(serde_json::from_str::<SettingsPatch>(r#"{"dailyCap":5000000}"#).is_err());
    }

    #[test]
    fn policy_fields_persist_and_old_files_still_load() {
        let path = tmp_path();
        let s = SettingsStore::load(path.clone(), &prices());
        let dir = std::env::temp_dir();
        let patch = SettingsPatch {
            allowed_folders: Some(vec![dir.to_string_lossy().into_owned()]),
            allowed_apps: Some(vec![
                app("com.spotify.client", "SPOOF"),
                app("com.spotify.client", "x"),
                app("calc", ""),
            ]),
            auto_allow_medium: Some(true),
            ..Default::default()
        };
        let out = s.apply_with(&patch, &prices(), &installed()).unwrap();
        // Names come from the installed list, ids are deduplicated.
        assert_eq!(
            out.allowed_apps,
            vec![
                app("com.spotify.client", "Spotify"),
                app("calc", "Calculator")
            ]
        );
        assert!(out.auto_allow_medium && out.allowed_folders.len() == 1);
        assert_eq!(SettingsStore::load(path, &prices()).get(), out);
        // A file written before these fields existed loads with empty lists.
        let old = tmp_path();
        std::fs::write(&old, r#"{"schemaVersion":1,"model":"claude-haiku-5-5","dailyCapMicroUsd":null,"monthlyCapMicroUsd":null}"#).unwrap();
        let o = SettingsStore::load(old, &prices()).get();
        assert!(o.allowed_folders.is_empty() && o.allowed_apps.is_empty() && !o.auto_allow_medium);
    }

    #[test]
    fn folders_must_be_absolute_existing_directories_and_are_canonical() {
        let s = SettingsStore::load(tmp_path(), &prices());
        let base = std::env::temp_dir().join(format!("jarvis-dir-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(base.join("inner")).unwrap();
        let file = base.join("file.txt");
        std::fs::write(&file, "x").unwrap();
        let with_dots = base.join("inner").join("..").join("inner");
        let ok = s
            .apply(
                &SettingsPatch {
                    allowed_folders: Some(vec![with_dots.to_string_lossy().into_owned()]),
                    ..Default::default()
                },
                &prices(),
            )
            .unwrap();
        assert!(
            !ok.allowed_folders[0].contains(".."),
            "{:?}",
            ok.allowed_folders
        );
        for bad in [
            "relative/dir",
            &file.to_string_lossy(),
            &base.join("missing").to_string_lossy(),
        ] {
            let r = s.apply(
                &SettingsPatch {
                    allowed_folders: Some(vec![bad.to_string()]),
                    ..Default::default()
                },
                &prices(),
            );
            assert!(
                matches!(r, Err(SettingsError::BadFolder(_))),
                "{bad}: {r:?}"
            );
        }
        // A rejected patch leaves the earlier list in place.
        assert_eq!(s.get().allowed_folders, ok.allowed_folders);
    }

    #[test]
    fn only_installed_app_ids_can_be_allowed() {
        let s = SettingsStore::load(tmp_path(), &prices());
        let patch = |apps: Vec<AllowedApp>| SettingsPatch {
            allowed_apps: Some(apps),
            ..Default::default()
        };
        // An id that is not installed, or an app known only by name, is refused.
        for bad in [app("com.evil.fake", "Spotify"), app("Spotify", "Spotify")] {
            let r = s.apply_with(&patch(vec![bad.clone()]), &prices(), &installed());
            assert_eq!(r, Err(SettingsError::UnknownApp(bad.id.clone())), "{bad:?}");
        }
        // With no installed-app list at all, nothing can be allowed.
        assert!(matches!(
            s.apply(&patch(vec![app("calc", "Calculator")]), &prices()),
            Err(SettingsError::UnknownApp(_))
        ));
        // Too many entries.
        let many: Vec<AllowedApp> = (0..=MAX_ALLOWED_APPS)
            .map(|i| app(&format!("a{i}"), "x"))
            .collect();
        let big: Vec<AllowedApp> = many.clone();
        assert_eq!(
            s.apply_with(&patch(many), &prices(), &big),
            Err(SettingsError::TooMany)
        );
        // Clearing is always valid, and a rejected patch left the list as it was.
        assert!(s.get().allowed_apps.is_empty());
        s.apply_with(&patch(vec![app("calc", "")]), &prices(), &installed())
            .unwrap();
        assert!(
            s.apply(&patch(vec![]), &prices())
                .unwrap()
                .allowed_apps
                .is_empty()
        );
    }
}
