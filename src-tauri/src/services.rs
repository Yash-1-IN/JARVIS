//! The settings, API key, usage and provider logic behind the settings commands.
//! Plain Rust with no Tauri types, so every rule here is unit-tested.

use crate::config::{SettingsError, SettingsStore};
use crate::intelligence::anthropic::{AnthropicConfig, AnthropicProvider, check_key};
use crate::intelligence::pricing::{DEFAULT_MODEL, PriceTable};
use crate::intelligence::provider::{ModelProvider, ProviderError};
use crate::intelligence::spend::{Caps, Day, SpendMeter};
use crate::ipc::types::{
    ApiKeyStatus, ErrorCode, JarvisError, KeyStorage, Settings, SettingsPatch, UsageSummary,
};
use crate::orchestrator::turn::TurnConfig;
use crate::platform::keychain::{ApiKeyVault, KeyStorage as VaultStorage, SecretString};
use std::sync::Arc;
use std::time::Duration;

/// How long `api_key_set` waits for the provider before giving up (#8).
pub const KEY_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Services {
    pub settings: Arc<SettingsStore>,
    pub prices: Arc<PriceTable>,
    pub vault: Arc<ApiKeyVault>,
    pub spend: Arc<SpendMeter>,
    /// `https://api.anthropic.com` in the app; a fake server in tests.
    pub api_base_url: String,
}

fn err(code: ErrorCode, message: &str, retryable: bool) -> JarvisError {
    JarvisError {
        code,
        message: message.into(),
        retryable,
        request_id: None,
    }
}

fn map_storage(s: VaultStorage) -> KeyStorage {
    match s {
        VaultStorage::Keychain => KeyStorage::Keychain,
        VaultStorage::SessionOnly => KeyStorage::SessionOnly,
    }
}

impl Services {
    /// Applies the saved spend caps to the meter. Call once at start-up.
    pub fn apply_saved_caps(&self) {
        let s = self.settings.get();
        self.spend.set_caps(Caps {
            daily: s.daily_cap_micro_usd,
            monthly: s.monthly_cap_micro_usd,
        });
    }

    pub fn key_status(&self) -> ApiKeyStatus {
        match self.vault.get() {
            Ok(Some(k)) => ApiKeyStatus {
                is_set: true,
                storage: Some(map_storage(self.vault.storage())),
                hint: k.hint(),
            },
            _ => ApiKeyStatus {
                is_set: false,
                storage: None,
                hint: None,
            },
        }
    }

    pub fn settings(&self) -> Settings {
        let s = self.settings.get();
        Settings {
            schema_version: s.schema_version,
            api_key: self.key_status(),
            model: s.model,
            available_models: self.prices.model_ids(),
            daily_cap_micro_usd: s.daily_cap_micro_usd,
            monthly_cap_micro_usd: s.monthly_cap_micro_usd,
        }
    }

    pub fn set_settings(&self, patch: &SettingsPatch) -> Result<Settings, JarvisError> {
        self.settings
            .apply(patch, &self.prices)
            .map_err(|e| match e {
                SettingsError::UnknownModel(_) | SettingsError::CapTooLarge => {
                    err(ErrorCode::Internal, &e.to_string(), false)
                }
                SettingsError::Save => {
                    err(ErrorCode::Internal, "Settings could not be saved.", true)
                }
            })?;
        self.apply_saved_caps();
        Ok(self.settings())
    }

    pub fn usage(&self, now: Day) -> UsageSummary {
        self.spend.summary(now)
    }

    /// Validates the key against the provider and stores it only if accepted, so a
    /// rejected or unverifiable key is never kept (#8). Never logs the key.
    pub async fn set_api_key(&self, raw: &str) -> Result<ApiKeyStatus, JarvisError> {
        let key = SecretString::parse(raw).map_err(|_| {
            err(
                ErrorCode::ProviderAuth,
                "That does not look like an API key. Check for spaces and paste it again.",
                false,
            )
        })?;
        match check_key(&self.api_base_url, key.expose(), KEY_CHECK_TIMEOUT).await {
            Ok(()) => {}
            Err(ProviderError::Auth) => {
                return Err(err(
                    ErrorCode::ProviderAuth,
                    "The provider rejected that key.",
                    false,
                ));
            }
            Err(ProviderError::Offline) => {
                return Err(err(
                    ErrorCode::NetworkOffline,
                    "No connection, so the key could not be checked. It was not saved.",
                    true,
                ));
            }
            Err(ProviderError::RateLimit) => {
                return Err(err(
                    ErrorCode::ProviderRateLimit,
                    "The provider is busy. The key was not saved; try again.",
                    true,
                ));
            }
            Err(_) => {
                return Err(err(
                    ErrorCode::ProviderUnavailable,
                    "The provider did not answer, so the key could not be checked. It was not saved.",
                    true,
                ));
            }
        }
        self.vault
            .set(&key)
            .map_err(|_| err(ErrorCode::Internal, "The key could not be stored.", true))?;
        Ok(self.key_status())
    }

    pub fn clear_api_key(&self) -> Result<ApiKeyStatus, JarvisError> {
        self.vault
            .clear()
            .map_err(|_| err(ErrorCode::Internal, "The key could not be removed.", true))?;
        Ok(self.key_status())
    }

    /// A provider for the stored key, or `None` when no key is set.
    pub fn provider(&self) -> Option<Arc<dyn ModelProvider>> {
        let key = self.vault.get().ok().flatten()?;
        let mut cfg = AnthropicConfig::new(key.expose().to_owned());
        cfg.base_url = self.api_base_url.clone();
        AnthropicProvider::new(cfg)
            .ok()
            .map(|p| Arc::new(p) as Arc<dyn ModelProvider>)
    }

    /// Model and price for the next turn, read from the current settings.
    pub fn turn_config(&self) -> TurnConfig {
        let model = self.settings.get().model;
        match self.prices.price(&model) {
            Ok(price) => TurnConfig { model, price },
            // Settings are validated against the table, so this is only a safety net.
            Err(_) => TurnConfig {
                model: DEFAULT_MODEL.into(),
                price: self
                    .prices
                    .price(DEFAULT_MODEL)
                    .expect("default model is priced"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::keychain::{KeyStorage as VS, SessionMemory};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const KEY: &str = "sk-ant-test-0123456789abcdefWXYZ";
    const DAY: Day = Day {
        year: 2026,
        month: 11,
        day: 1,
    };

    fn services(api: &str) -> Services {
        let path = std::env::temp_dir()
            .join(format!("jarvis-svc-{}", uuid::Uuid::now_v7()))
            .join("settings.json");
        let prices = Arc::new(PriceTable::embedded().unwrap());
        Services {
            settings: Arc::new(SettingsStore::load(path, &prices)),
            prices,
            vault: Arc::new(ApiKeyVault::with_backend(
                Box::<SessionMemory>::default(),
                VS::SessionOnly,
            )),
            spend: Arc::new(SpendMeter::new(Caps::default(), DAY)),
            api_base_url: api.into(),
        }
    }

    /// Serves one canned status for a single GET, then stops.
    async fn one_shot(status: u16) -> String {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf).await;
            let r = format!(
                "HTTP/1.1 {status} X\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}"
            );
            let _ = s.write_all(r.as_bytes()).await;
            let _ = s.shutdown().await;
        });
        url
    }

    #[tokio::test]
    async fn accepted_key_is_stored_and_reported_without_the_key() {
        let svc = services(&one_shot(200).await);
        assert!(!svc.key_status().is_set);
        let st = svc.set_api_key(&format!("  {KEY}\n")).await.unwrap();
        assert_eq!(
            (st.is_set, st.storage, st.hint.as_deref()),
            (true, Some(KeyStorage::SessionOnly), Some("…WXYZ"))
        );
        let shown = format!("{:?}", svc.settings());
        assert!(!shown.contains("sk-ant"), "{shown}");
        assert!(svc.provider().is_some());
    }

    #[tokio::test]
    async fn rejected_key_is_not_stored() {
        let svc = services(&one_shot(401).await);
        let e = svc.set_api_key(KEY).await.unwrap_err();
        assert_eq!((e.code, e.retryable), (ErrorCode::ProviderAuth, false));
        assert!(!svc.key_status().is_set);
        assert!(svc.provider().is_none());
    }

    #[tokio::test]
    async fn offline_key_check_stores_nothing() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead = format!("http://{}", l.local_addr().unwrap());
        drop(l);
        let svc = services(&dead);
        let e = svc.set_api_key(KEY).await.unwrap_err();
        assert_eq!((e.code, e.retryable), (ErrorCode::NetworkOffline, true));
        assert!(!svc.key_status().is_set);
    }

    #[tokio::test]
    async fn malformed_key_never_reaches_the_network() {
        let svc = services("http://127.0.0.1:1");
        for bad in ["", "two words", &"x".repeat(600)] {
            let e = svc.set_api_key(bad).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::ProviderAuth, "{bad:?}");
            assert!(!e.message.contains(bad) || bad.is_empty());
        }
    }

    #[tokio::test]
    async fn clearing_removes_the_key() {
        let svc = services(&one_shot(200).await);
        svc.set_api_key(KEY).await.unwrap();
        let st = svc.clear_api_key().unwrap();
        assert!(!st.is_set && st.hint.is_none() && st.storage.is_none());
        assert!(svc.provider().is_none());
    }

    #[test]
    fn settings_lists_priced_models_and_caps_reach_the_meter() {
        let svc = services("http://unused");
        let s = svc.settings();
        assert!(s.available_models.contains(&s.model));
        let patch: SettingsPatch =
            serde_json::from_str(r#"{"dailyCapMicroUsd":1000,"model":"claude-sonnet-5-5"}"#)
                .unwrap();
        let out = svc.set_settings(&patch).unwrap();
        assert_eq!(
            (out.model.as_str(), out.daily_cap_micro_usd),
            ("claude-sonnet-5-5", Some(1000))
        );
        assert_eq!(svc.usage(DAY).daily_cap_micro_usd, Some(1000));
        assert_eq!(svc.turn_config().model, "claude-sonnet-5-5");
        let clear: SettingsPatch = serde_json::from_str(r#"{"dailyCapMicroUsd":null}"#).unwrap();
        svc.set_settings(&clear).unwrap();
        assert_eq!(svc.usage(DAY).daily_cap_micro_usd, None);
    }

    #[test]
    fn invalid_settings_are_rejected_with_no_change() {
        let svc = services("http://unused");
        let bad: SettingsPatch = serde_json::from_str(r#"{"model":"gpt-4"}"#).unwrap();
        let e = svc.set_settings(&bad).unwrap_err();
        assert!(e.message.contains("gpt-4"));
        assert_eq!(svc.settings().model, DEFAULT_MODEL);
    }

    #[test]
    fn saved_caps_are_applied_at_startup() {
        let svc = services("http://unused");
        let p: SettingsPatch = serde_json::from_str(r#"{"monthlyCapMicroUsd":777}"#).unwrap();
        svc.settings.apply(&p, &svc.prices).unwrap();
        assert_eq!(svc.spend.caps().monthly, None);
        svc.apply_saved_caps();
        assert_eq!(svc.spend.caps().monthly, Some(777));
    }
}
