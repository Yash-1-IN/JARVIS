//! Settings persisted as JSON in the app config directory (spec §8.2 `settings`, versioned).
//! The API key is never stored here; it lives in the keychain vault.

use crate::intelligence::pricing::{DEFAULT_MODEL, PriceTable};
use crate::ipc::types::SettingsPatch;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub const SCHEMA_VERSION: u32 = 1;
/// Sanity limit for a spend cap: $100,000 in micro-dollars.
pub const MAX_CAP_MICRO_USD: u64 = 100_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSettings {
    pub schema_version: u32,
    pub model: String,
    pub daily_cap_micro_usd: Option<u64>,
    pub monthly_cap_micro_usd: Option<u64>,
}

impl Default for StoredSettings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            model: DEFAULT_MODEL.into(),
            daily_cap_micro_usd: None,
            monthly_cap_micro_usd: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("`{0}` is not a supported model")]
    UnknownModel(String),
    #[error("a spend cap may not exceed $100,000")]
    CapTooLarge,
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
    pub fn apply(
        &self,
        patch: &SettingsPatch,
        prices: &PriceTable,
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
}
