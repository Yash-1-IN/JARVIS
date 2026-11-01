mod intelligence;
mod ipc;
mod logging;
mod orchestrator;

use intelligence::pricing::{DEFAULT_MODEL, PriceTable};
use intelligence::spend::{Caps, Day, SpendMeter};
use orchestrator::turn::{TurnConfig, TurnRunner};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let orch = Arc::new(orchestrator::Orchestrator::default());
    let now_secs = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    };
    let spend = Arc::new(SpendMeter::new(
        Caps::default(),
        Day::from_unix_secs(now_secs()),
    ));
    // No provider until the API key is wired (settings contract, #8): turns that need
    // the cloud report PROVIDER_AUTH; local commands work.
    let default_price = PriceTable::embedded()
        .and_then(|t| t.price(DEFAULT_MODEL))
        .expect("the embedded pricing table must price the default model");
    let runner = Arc::new(TurnRunner::new(
        orch.clone(),
        spend,
        Box::new(|| None),
        // TODO(#11 wiring): take the model from `settings_get().model` once settings exist.
        TurnConfig {
            model: DEFAULT_MODEL.into(),
            price: default_price,
        },
        Box::new(now_secs),
    ));
    tauri::Builder::default()
        .manage(orch)
        .manage(runner)
        .setup(|app| {
            let dir = app.path().app_log_dir()?;
            std::fs::create_dir_all(&dir)?;
            // Keep the guard for the app's lifetime so buffered logs flush.
            let guard = logging::init(&dir)?;
            app.manage(guard);
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "jarvis started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::commands::ping,
            ipc::commands::get_snapshot,
            ipc::commands::set_mute,
            ipc::commands::cancel,
            ipc::commands::submit_text
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
