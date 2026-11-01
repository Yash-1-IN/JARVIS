mod config;
mod intelligence;
mod ipc;
mod logging;
mod orchestrator;
mod platform;
mod services;

use config::SettingsStore;
use intelligence::pricing::PriceTable;
use intelligence::spend::{Caps, Day, SpendMeter};
use orchestrator::turn::TurnRunner;
use platform::keychain::ApiKeyVault;
use services::Services;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

const ANTHROPIC_API: &str = "https://api.anthropic.com";

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let log_dir = app.path().app_log_dir()?;
            std::fs::create_dir_all(&log_dir)?;
            // Keep the guard for the app's lifetime so buffered logs flush.
            let guard = logging::init(&log_dir)?;
            app.manage(guard);
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "jarvis started");

            let prices = Arc::new(
                PriceTable::embedded().map_err(|e| format!("pricing table is invalid: {e}"))?,
            );
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let svc = Arc::new(Services {
                settings: Arc::new(SettingsStore::load(settings_path, &prices)),
                prices,
                vault: Arc::new(ApiKeyVault::open()),
                spend: Arc::new(SpendMeter::new(
                    Caps::default(),
                    Day::from_unix_secs(now_secs()),
                )),
                api_base_url: ANTHROPIC_API.into(),
                // TODO(Dev B, platform/): the per-OS installed-apps list; until then no app can be allowed.
                installed: Arc::new(platform::apps::PlatformApps::new()),
            });
            svc.apply_saved_caps();

            let orch = Arc::new(orchestrator::Orchestrator::default());
            let (for_provider, for_config) = (svc.clone(), svc.clone());
            let runner = Arc::new(TurnRunner::new(
                orch.clone(),
                svc.spend.clone(),
                // Cloud turns read the stored key each time; no key means PROVIDER_AUTH.
                Box::new(move || for_provider.provider()),
                // The model and its price come from the current settings (D-16).
                Box::new(move || for_config.turn_config()),
                Box::new(now_secs),
            ));
            app.manage(orch);
            app.manage(runner);
            app.manage(svc);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::commands::ping,
            ipc::commands::get_snapshot,
            ipc::commands::set_mute,
            ipc::commands::cancel,
            ipc::commands::submit_text,
            ipc::commands::settings_get,
            ipc::commands::settings_set,
            ipc::commands::usage_get,
            ipc::commands::api_key_set,
            ipc::commands::api_key_clear,
            ipc::commands::apps_installed
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
