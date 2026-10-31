use super::{PROTOCOL_VERSION, types::*};
use crate::orchestrator::Orchestrator;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Builds the `state.snapshot` envelope; its seq is the snapshot's seq.
pub fn snapshot_envelope(s: Snapshot) -> Envelope<Snapshot> {
    Envelope {
        v: PROTOCOL_VERSION,
        seq: s.seq,
        id: uuid::Uuid::now_v7().to_string(),
        ts: now_ms(),
        kind: "state.snapshot".into(),
        payload: s,
    }
}

/// Tauri event names cannot contain '.', so `state.snapshot` is emitted as `state_snapshot`.
pub fn publish(app: &tauri::AppHandle, s: Snapshot) {
    let env = snapshot_envelope(s);
    if let Err(e) = app.emit(&env.kind.replace('.', "_"), &env) {
        tracing::warn!(error = %e, "failed to emit state.snapshot");
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
pub fn get_snapshot(state: tauri::State<'_, Orchestrator>) -> Snapshot {
    state.snapshot()
}

#[tauri::command]
pub fn set_mute(app: tauri::AppHandle, state: tauri::State<'_, Orchestrator>, muted: bool) {
    publish(&app, state.set_mute(muted));
}

#[tauri::command]
pub fn cancel(
    app: tauri::AppHandle,
    state: tauri::State<'_, Orchestrator>,
    request_id: String,
) -> Result<(), JarvisError> {
    match state.cancel(&request_id) {
        Ok(s) => {
            publish(&app, s);
            Ok(())
        }
        Err(_) => Err(JarvisError {
            code: ErrorCode::Internal,
            message: "no active turn with that request id".into(),
            retryable: false,
            request_id: Some(request_id),
        }),
    }
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
    fn snapshot_envelope_uses_snapshot_seq_and_kind() {
        let o = Orchestrator::default();
        let e = snapshot_envelope(o.set_mute(true));
        assert_eq!(
            (e.kind.as_str(), e.seq, e.v),
            ("state.snapshot", 1, PROTOCOL_VERSION)
        );
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["payload"]["mic"], "muted");
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
