use super::{PROTOCOL_VERSION, types::*};
use std::sync::Mutex;

/// Authoritative state; later owned by the orchestrator (M2+).
pub struct AppState(pub Mutex<Snapshot>);

impl Default for AppState {
    fn default() -> Self {
        Self(Mutex::new(Snapshot::initial()))
    }
}

pub fn ping_response() -> PingResponse {
    PingResponse {
        pong: true,
        protocol_version: PROTOCOL_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

#[tauri::command]
pub fn ping() -> PingResponse {
    ping_response()
}

#[tauri::command]
pub fn get_snapshot(state: tauri::State<'_, AppState>) -> Snapshot {
    state.0.lock().expect("state lock poisoned").clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_reports_protocol_version() {
        let p = ping_response();
        assert!(p.pong);
        assert_eq!(p.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn snapshot_serialises_camel_case() {
        let v = serde_json::to_value(Snapshot::initial()).unwrap();
        assert_eq!(v["mic"], "off");
        assert_eq!(v["phase"], "idle");
        assert!(v.get("activeRequestId").is_some());
        assert!(v.get("pendingApprovals").is_some());
    }
}
