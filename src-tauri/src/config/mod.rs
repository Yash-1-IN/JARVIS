//! Settings persisted as JSON in the app config directory (spec §8.2 `settings`, versioned).
//! The API key is never stored here; it lives in the keychain vault.

use crate::intelligence::pricing::{DEFAULT_MODEL, PriceTable};
use crate::ipc::types::{AllowedApp, SettingsPatch, VoiceSettingsPatch};
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
    /// The Spotify developer app's Client ID (not a secret). Older files lack this field.
    #[serde(default)]
    pub spotify_client_id: Option<String>,
    /// Voice choices (M3). Older files lack this field.
    #[serde(default)]
    pub voice: StoredVoice,
}

/// The saved voice choices. What is installed, and the OS permission, are read live, not saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredVoice {
    #[serde(default)]
    pub wake_enabled: bool,
    #[serde(default = "yes")]
    pub follow_up_window: bool,
    #[serde(default = "default_voice")]
    pub voice: String,
    #[serde(default)]
    pub input_device: Option<String>,
    /// The wake word's licence notice was accepted. Older files lack this field: not accepted.
    #[serde(default)]
    pub wake_notice_accepted: bool,
}

fn yes() -> bool {
    true
}

/// The voice used until the person picks another.
pub const DEFAULT_VOICE: &str = "default";

fn default_voice() -> String {
    DEFAULT_VOICE.into()
}

impl Default for StoredVoice {
    fn default() -> Self {
        Self {
            wake_enabled: false,
            follow_up_window: true,
            voice: default_voice(),
            input_device: None,
            wake_notice_accepted: false,
        }
    }
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
            spotify_client_id: None,
            voice: StoredVoice::default(),
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
    #[error("that does not look like a Spotify Client ID (32 characters, 0-9 and a-f)")]
    BadClientId,
    #[error("read and accept the wake word notice first")]
    WakeNoticeNeeded,
    #[error("that is not a valid voice id (1 to 64 letters, digits, `_` or `-`)")]
    BadVoice,
    #[error("that is not a valid microphone name (1 to 200 characters, no control characters)")]
    BadDevice,
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

    /// Sets (or, with `None` or an empty string, clears) the Spotify Client ID, then saves.
    /// On any error nothing changes.
    pub fn set_spotify_client_id(&self, id: Option<&str>) -> Result<Option<String>, SettingsError> {
        let mut next = self.get();
        next.spotify_client_id = match id {
            Some(raw) => valid_client_id(raw)?,
            None => None,
        };
        save_atomically(&self.path, &next).map_err(|e| {
            tracing::warn!(error = %e, "saving settings failed");
            SettingsError::Save
        })?;
        let saved = next.spotify_client_id.clone();
        *self.lock() = next;
        Ok(saved)
    }
}

impl SettingsStore {
    /// Validates and applies a voice patch, then saves. On any error nothing changes.
    pub fn apply_voice(&self, patch: &VoiceSettingsPatch) -> Result<StoredVoice, SettingsError> {
        let mut next = self.get();
        if let Some(on) = patch.wake_notice_accepted {
            next.voice.wake_notice_accepted = on;
        }
        if let Some(on) = patch.wake_enabled {
            next.voice.wake_enabled = on;
        }
        // The wake word listens all the time, so it cannot be on without the licence notice having
        // been accepted. Only checked when this patch touches one of the two, so a saved file that
        // predates the notice does not block unrelated changes.
        if (patch.wake_enabled.is_some() || patch.wake_notice_accepted.is_some())
            && next.voice.wake_enabled
            && !next.voice.wake_notice_accepted
        {
            return Err(SettingsError::WakeNoticeNeeded);
        }
        if let Some(on) = patch.follow_up_window {
            next.voice.follow_up_window = on;
        }
        if let Some(v) = &patch.voice {
            next.voice.voice = valid_voice_id(v)?;
        }
        if let Some(d) = &patch.input_device {
            next.voice.input_device = match d {
                Some(name) => valid_device_name(name)?,
                None => None,
            };
        }
        save_atomically(&self.path, &next).map_err(|e| {
            tracing::warn!(error = %e, "saving settings failed");
            SettingsError::Save
        })?;
        let saved = next.voice.clone();
        *self.lock() = next;
        Ok(saved)
    }
}

/// A voice id is 1 to 64 ASCII letters, digits, `_` or `-`. Whether it is installed is checked
/// where the voices are known, not here.
pub fn valid_voice_id(input: &str) -> Result<String, SettingsError> {
    let id = input.trim();
    if !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        Ok(id.to_string())
    } else {
        Err(SettingsError::BadVoice)
    }
}

/// A microphone name is shown by the system, so it can hold spaces, brackets and non-ASCII text:
/// only its length and control characters are checked. Empty means "the system default".
pub fn valid_device_name(input: &str) -> Result<Option<String>, SettingsError> {
    let name = input.trim();
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().count() <= 200 && !name.chars().any(char::is_control) {
        Ok(Some(name.to_string()))
    } else {
        Err(SettingsError::BadDevice)
    }
}

/// A Spotify Client ID is 32 hexadecimal characters. Stored lower case; empty means "clear it".
pub fn valid_client_id(input: &str) -> Result<Option<String>, SettingsError> {
    let id = input.trim().to_ascii_lowercase();
    if id.is_empty() {
        return Ok(None);
    }
    if id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(Some(id))
    } else {
        Err(SettingsError::BadClientId)
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
pub(crate) fn display_path(p: &Path) -> String {
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

    const ID: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn a_spotify_client_id_is_32_hex_characters_and_nothing_else() {
        assert_eq!(valid_client_id(ID), Ok(Some(ID.to_string())));
        // Trimmed, and stored lower case.
        assert_eq!(
            valid_client_id("  0123456789ABCDEF0123456789ABCDEF \n"),
            Ok(Some(ID.to_string()))
        );
        // Empty clears it.
        assert_eq!(valid_client_id(""), Ok(None));
        assert_eq!(valid_client_id("   "), Ok(None));
        for bad in [
            "0123456789abcdef0123456789abcde",       // 31
            "0123456789abcdef0123456789abcdef0",     // 33
            "0123456789abcdeg0123456789abcdef",      // not hex
            "0123456789abcdef0123456789abcde\u{e9}", // not ASCII
            "client_id=0123456789abcdef0123456789abcdef",
            "https://accounts.spotify.com/authorize?x=1",
            "0123456789abcdef 0123456789abcdef",
        ] {
            assert_eq!(
                valid_client_id(bad),
                Err(SettingsError::BadClientId),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn the_client_id_is_saved_cleared_and_survives_a_restart() {
        let path = tmp_path();
        let store = SettingsStore::load(path.clone(), &prices());
        assert_eq!(store.get().spotify_client_id, None);
        assert_eq!(
            store.set_spotify_client_id(Some(ID)),
            Ok(Some(ID.to_string()))
        );
        let again = SettingsStore::load(path.clone(), &prices());
        assert_eq!(again.get().spotify_client_id.as_deref(), Some(ID));
        // A bad value changes nothing, on disk or in memory.
        assert_eq!(
            again.set_spotify_client_id(Some("nope")),
            Err(SettingsError::BadClientId)
        );
        assert_eq!(again.get().spotify_client_id.as_deref(), Some(ID));
        assert_eq!(
            SettingsStore::load(path.clone(), &prices())
                .get()
                .spotify_client_id
                .as_deref(),
            Some(ID)
        );
        // None and an empty string both clear it.
        assert_eq!(again.set_spotify_client_id(None), Ok(None));
        assert_eq!(
            again.set_spotify_client_id(Some(ID)),
            Ok(Some(ID.to_string()))
        );
        assert_eq!(again.set_spotify_client_id(Some("")), Ok(None));
        assert_eq!(
            SettingsStore::load(path, &prices()).get().spotify_client_id,
            None
        );
    }

    #[test]
    fn a_settings_file_from_before_spotify_still_loads() {
        let path = tmp_path();
        std::fs::write(
            &path,
            r#"{"schemaVersion":1,"model":"claude-haiku-4-5","dailyCapMicroUsd":null,"monthlyCapMicroUsd":null}"#,
        )
        .unwrap();
        let store = SettingsStore::load(path, &prices());
        assert_eq!(store.get().spotify_client_id, None);
        // ... and has the voice defaults: wake word off, follow-up window on.
        assert_eq!(store.get().voice, StoredVoice::default());
        assert!(!store.get().voice.wake_enabled);
        assert!(store.get().voice.follow_up_window);
    }

    #[test]
    fn voice_ids_and_device_names_are_checked_and_never_echoed() {
        assert_eq!(valid_voice_id(" af_sky-2 "), Ok("af_sky-2".to_string()));
        for bad in ["", "  ", "a b", "../x", "vo\nice", "é", &"a".repeat(65)] {
            assert_eq!(valid_voice_id(bad), Err(SettingsError::BadVoice), "{bad:?}");
        }
        assert_eq!(valid_device_name(""), Ok(None));
        assert_eq!(
            valid_device_name("Microphone Array (Realtek(R) Audio) é"),
            Ok(Some("Microphone Array (Realtek(R) Audio) é".to_string()))
        );
        for bad in ["a\0b", "a\nb", &"x".repeat(201)] {
            assert_eq!(
                valid_device_name(bad),
                Err(SettingsError::BadDevice),
                "{bad:?}"
            );
        }
        // The messages describe the rule, not the input.
        assert!(!SettingsError::BadVoice.to_string().contains("../"));
    }

    #[test]
    fn the_wake_word_cannot_be_turned_on_without_the_notice() {
        let store = SettingsStore::load(tmp_path(), &prices());
        let on = VoiceSettingsPatch {
            wake_enabled: Some(true),
            ..Default::default()
        };
        assert_eq!(store.apply_voice(&on), Err(SettingsError::WakeNoticeNeeded));
        assert!(!store.get().voice.wake_enabled && !store.get().voice.wake_notice_accepted);
        // Accepting and enabling in one patch works, and survives a restart.
        let both = VoiceSettingsPatch {
            wake_enabled: Some(true),
            wake_notice_accepted: Some(true),
            ..Default::default()
        };
        assert!(store.apply_voice(&both).unwrap().wake_enabled);
        // Withdrawing the notice while the wake word is on is refused, and changes nothing.
        let withdraw = VoiceSettingsPatch {
            wake_notice_accepted: Some(false),
            ..Default::default()
        };
        assert_eq!(
            store.apply_voice(&withdraw),
            Err(SettingsError::WakeNoticeNeeded)
        );
        assert!(store.get().voice.wake_notice_accepted);
        // Turning it off first, then withdrawing, is fine.
        let off = VoiceSettingsPatch {
            wake_enabled: Some(false),
            wake_notice_accepted: Some(false),
            ..Default::default()
        };
        let s = store.apply_voice(&off).unwrap();
        assert!(!s.wake_enabled && !s.wake_notice_accepted);
    }

    #[test]
    fn a_file_saved_before_the_notice_loads_as_not_accepted_and_does_not_block_other_changes() {
        let path = tmp_path();
        std::fs::write(
            &path,
            r#"{"schemaVersion":1,"model":"claude-haiku-5-5","dailyCapMicroUsd":null,"monthlyCapMicroUsd":null,"voice":{"wakeEnabled":true}}"#,
        )
        .unwrap();
        let store = SettingsStore::load(path, &prices());
        assert!(store.get().voice.wake_enabled && !store.get().voice.wake_notice_accepted);
        let other = VoiceSettingsPatch {
            follow_up_window: Some(false),
            ..Default::default()
        };
        assert!(store.apply_voice(&other).is_ok());
    }

    #[test]
    fn voice_settings_are_saved_survive_a_restart_and_a_bad_value_changes_nothing() {
        let path = tmp_path();
        let store = SettingsStore::load(path.clone(), &prices());
        let patch = VoiceSettingsPatch {
            wake_enabled: Some(true),
            follow_up_window: Some(false),
            voice: Some("af_sky".into()),
            input_device: Some(Some("Realtek".into())),
            wake_notice_accepted: Some(true),
        };
        let saved = store.apply_voice(&patch).unwrap();
        assert!(saved.wake_enabled && !saved.follow_up_window);
        let again = SettingsStore::load(path.clone(), &prices());
        assert_eq!(again.get().voice, saved);
        // A bad voice in a patch with good fields applies none of them.
        let bad = VoiceSettingsPatch {
            wake_enabled: Some(false),
            voice: Some("a b".into()),
            ..Default::default()
        };
        assert_eq!(again.apply_voice(&bad), Err(SettingsError::BadVoice));
        assert_eq!(again.get().voice, saved);
        assert_eq!(
            SettingsStore::load(path.clone(), &prices()).get().voice,
            saved
        );
        // `null` goes back to the system default; an empty patch changes nothing.
        let clear = VoiceSettingsPatch {
            input_device: Some(None),
            ..Default::default()
        };
        assert_eq!(again.apply_voice(&clear).unwrap().input_device, None);
        let same = again.apply_voice(&VoiceSettingsPatch::default()).unwrap();
        assert_eq!(same.voice, "af_sky");
    }
}
