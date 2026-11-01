use super::{PROTOCOL_VERSION, types::*};
use crate::intelligence::spend::Day;
use crate::orchestrator::Orchestrator;
use crate::orchestrator::turn::{TurnRunner, TurnSink};
use crate::services::Services;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tauri::ipc::Channel;

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
        kind: EventKind::StateSnapshot,
        payload: s,
    }
}

/// Tauri event names cannot contain '.', so `state.snapshot` is emitted as `state_snapshot`.
pub fn publish(app: &tauri::AppHandle, s: Snapshot) {
    let env = snapshot_envelope(s);
    if let Err(e) = app.emit(&env.kind.tauri_event_name(), &env) {
        tracing::warn!(error = %e, "failed to emit state.snapshot");
    }
}

/// Sequence for events that are not snapshots (snapshots carry their own `seq`).
static EVENT_SEQ: AtomicU64 = AtomicU64::new(0);

/// Emits a non-snapshot event as an `Envelope` on its Tauri event name.
pub fn publish_event<P: serde::Serialize + Clone>(
    app: &tauri::AppHandle,
    kind: EventKind,
    payload: P,
) {
    let env = Envelope {
        v: PROTOCOL_VERSION,
        seq: EVENT_SEQ.fetch_add(1, Ordering::Relaxed) + 1,
        id: uuid::Uuid::now_v7().to_string(),
        ts: now_ms(),
        kind,
        payload,
    };
    if let Err(e) = app.emit(&kind.tauri_event_name(), &env) {
        tracing::warn!(error = %e, event = kind.wire_name(), "failed to emit event");
    }
}

/// Emits `usage.updated` (Tauri event `usage_updated`).
pub fn publish_usage(app: &tauri::AppHandle, u: UsageSummary) {
    publish_event(app, EventKind::UsageUpdated, u);
}

fn today() -> Day {
    Day::from_unix_secs(now_ms() / 1000)
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
pub fn get_snapshot(state: tauri::State<'_, Arc<Orchestrator>>) -> Snapshot {
    state.snapshot()
}

#[tauri::command]
pub fn set_mute(app: tauri::AppHandle, state: tauri::State<'_, Arc<Orchestrator>>, muted: bool) {
    publish(&app, state.set_mute(muted));
}

/// Publishes snapshots to the webview and, if given, streams turn events on a Channel.
struct AppSink {
    app: tauri::AppHandle,
    channel: Option<Channel<TurnStreamEvent>>,
}

impl TurnSink for AppSink {
    fn event(&self, e: TurnStreamEvent) {
        if let Some(c) = &self.channel
            && let Err(err) = c.send(e)
        {
            tracing::debug!(error = %err, "turn channel closed");
        }
    }
    fn snapshot(&self, s: Snapshot) {
        publish(&self.app, s);
    }
    fn usage(&self, u: UsageSummary) {
        publish_usage(&self.app, u);
    }
    fn interrupted(&self, i: TurnInterrupted) {
        publish_event(&self.app, EventKind::TurnInterrupted, i);
    }
}

#[tauri::command]
pub fn cancel(
    app: tauri::AppHandle,
    runner: tauri::State<'_, Arc<TurnRunner>>,
    request_id: String,
) {
    // Idempotent: Esc pressed just as a turn finishes is a no-op, not an error.
    let sink = AppSink { app, channel: None };
    if !runner.cancel(&request_id, InterruptReason::UserCancel, &sink) {
        tracing::debug!(request_id, "cancel for a turn that is not active");
    }
}

/// Starts a text turn and returns its request id at once; events stream on `on_event`
/// and always end with exactly one `done`.
// `async` so Tauri runs it on its Tokio runtime: a plain `fn` command runs on the main
// thread, which has no runtime, and `TurnRunner::start` spawns a task.
#[tauri::command]
pub async fn submit_text(
    app: tauri::AppHandle,
    runner: tauri::State<'_, Arc<TurnRunner>>,
    text: String,
    on_event: Channel<TurnStreamEvent>,
) -> Result<String, JarvisError> {
    let sink = Arc::new(AppSink {
        app,
        channel: Some(on_event),
    });
    runner.start(text, sink)
}

#[tauri::command]
pub fn settings_get(svc: tauri::State<'_, Arc<Services>>) -> Settings {
    svc.settings()
}

#[tauri::command]
pub fn settings_set(
    app: tauri::AppHandle,
    svc: tauri::State<'_, Arc<Services>>,
    patch: SettingsPatch,
) -> Result<Settings, JarvisError> {
    let out = svc.set_settings(&patch)?;
    publish_usage(&app, svc.usage(today())); // the caps changed
    Ok(out)
}

#[tauri::command]
pub fn usage_get(svc: tauri::State<'_, Arc<Services>>) -> UsageSummary {
    svc.usage(today())
}

// No `tracing::instrument` and no logging of arguments: `key` is the API key.
#[tauri::command]
pub async fn api_key_set(
    svc: tauri::State<'_, Arc<Services>>,
    key: String,
) -> Result<ApiKeyStatus, JarvisError> {
    svc.set_api_key(&key).await
}

#[tauri::command]
pub fn api_key_clear(svc: tauri::State<'_, Arc<Services>>) -> Result<ApiKeyStatus, JarvisError> {
    svc.clear_api_key()
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
            (e.kind, e.seq, e.v),
            (EventKind::StateSnapshot, 1, PROTOCOL_VERSION)
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
