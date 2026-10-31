mod ipc;
mod logging;
mod orchestrator;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(orchestrator::Orchestrator::default())
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
            ipc::commands::cancel
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
