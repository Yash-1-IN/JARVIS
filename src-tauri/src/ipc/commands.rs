use super::{PROTOCOL_VERSION, types::*};
use crate::intelligence::spend::Day;
use crate::memory::store::{MemoryStore, StoreError};
use crate::orchestrator::Orchestrator;
use crate::orchestrator::turn::{TurnRunner, TurnSink};
use crate::permissions::activity::ActivityLog;
use crate::permissions::gateway::Channel as ApprovalChannel;
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
    fn approval(&self, r: ApprovalRequest) {
        publish_event(&self.app, EventKind::ApprovalRequested, r);
    }
    fn activity(&self, e: ActivityEntry) {
        publish_event(&self.app, EventKind::ActivityAppended, e);
    }
}

/// Where a voice-started turn reports: the reply streams on the channel the window attached with
/// `voice_attach`, everything else is published as for a typed turn, and what was heard goes out
/// as `transcript.final`.
pub struct AppVoiceSink {
    pub app: tauri::AppHandle,
    pub stream: Arc<crate::ipc::voice_stream::VoiceStream>,
}

impl TurnSink for AppVoiceSink {
    fn event(&self, e: TurnStreamEvent) {
        if !self.stream.send(e) {
            tracing::debug!("no window is attached for the voice reply");
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
    fn approval(&self, r: ApprovalRequest) {
        publish_event(&self.app, EventKind::ApprovalRequested, r);
    }
    fn activity(&self, e: ActivityEntry) {
        publish_event(&self.app, EventKind::ActivityAppended, e);
    }
}

impl crate::voice::controller::VoiceSink for AppVoiceSink {
    fn transcript_final(&self, t: crate::ipc::types::Transcript) {
        publish_event(&self.app, EventKind::TranscriptFinal, t);
    }
}

/// Answers an approval card. Only the window can call this: voice has no route to it, and
/// the gateway independently refuses voice for high risk.
#[tauri::command]
pub fn resolve_approval(
    app: tauri::AppHandle,
    runner: tauri::State<'_, Arc<TurnRunner>>,
    decision: ApprovalDecision,
) -> Result<(), JarvisError> {
    let sink = AppSink { app, channel: None };
    runner
        .resolve_approval(decision, ApprovalChannel::Window, &sink)
        .map_err(|e| JarvisError {
            code: ErrorCode::ToolDenied,
            message: e.to_string(),
            retryable: false,
            request_id: None,
        })
}

/// Pages the activity log, newest first.
#[tauri::command]
pub fn activity_list(
    log: tauri::State<'_, Arc<ActivityLog>>,
    limit: usize,
    before: Option<String>,
) -> Vec<ActivityEntry> {
    log.list(limit, before.as_deref())
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

/// Runs a call that reads or writes the OS keychain on a blocking thread. macOS can show a
/// permission question while a keychain item is read; that must not freeze the window, which a
/// plain command (it runs on the main thread) would.
async fn off_main<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, JarvisError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| JarvisError {
            code: ErrorCode::Internal,
            message: "Spotify settings could not be read. Try again.".into(),
            retryable: true,
            request_id: None,
        })
}

/// The Spotify sign-in state (music step 2). Never carries a token. Off the main thread because
/// it reads the keychain; it fails only if that task cannot run.
#[tauri::command]
pub async fn spotify_status(
    svc: tauri::State<'_, Arc<Services>>,
) -> Result<SpotifyStatus, JarvisError> {
    let svc = svc.inner().clone();
    off_main(move || svc.spotify_status()).await
}

/// Saves the Client ID from the person's Spotify developer app (32 characters, 0-9 and a-f);
/// `null` or an empty string clears it. A bad value is an `INTERNAL` error with a plain message.
#[tauri::command]
pub async fn spotify_set_client_id(
    svc: tauri::State<'_, Arc<Services>>,
    client_id: Option<String>,
) -> Result<SpotifyStatus, JarvisError> {
    // Changing the Client ID also forgets the old sign-in, which clears the keychain.
    let svc = svc.inner().clone();
    off_main(move || svc.set_spotify_client_id(client_id.as_deref())).await?
}

/// Starts the sign-in in the person's browser and returns when it ends, times out (about 3
/// minutes) or is cancelled by `spotify_disconnect`. Async, so the window stays responsive.
#[tauri::command]
pub async fn spotify_connect(
    svc: tauri::State<'_, Arc<Services>>,
) -> Result<SpotifyStatus, JarvisError> {
    svc.spotify_connect().await
}

/// Forgets the Spotify sign-in (and cancels one in progress). Safe to call when not connected.
#[tauri::command]
pub async fn spotify_disconnect(
    svc: tauri::State<'_, Arc<Services>>,
) -> Result<SpotifyStatus, JarvisError> {
    let svc = svc.inner().clone();
    off_main(move || svc.spotify_disconnect()).await
}

/// Voice settings and state (M3). Text and settings only, never audio. `modelsReady` is read from the
/// models folder each time, so it is true as soon as a download has finished and verified.
#[tauri::command]
pub fn voice_status(
    svc: tauri::State<'_, Arc<Services>>,
    models: tauri::State<'_, Arc<crate::voice::models::ModelManager>>,
) -> VoiceStatus {
    VoiceStatus {
        models_ready: models.ready(),
        ..svc.voice_status()
    }
}

/// Saves voice choices. A bad value is an `INTERNAL` error with a plain message and nothing changes.
#[tauri::command]
pub fn voice_settings_set(
    svc: tauri::State<'_, Arc<Services>>,
    models: tauri::State<'_, Arc<crate::voice::models::ModelManager>>,
    patch: VoiceSettingsPatch,
) -> Result<VoiceStatus, JarvisError> {
    Ok(VoiceStatus {
        models_ready: models.ready(),
        ..svc.voice_settings_set(&patch)?
    })
}

/// The microphones and speakers the system has, by name, and which microphone is the default.
/// Asking the operating system can take a moment, so it runs off the main thread.
#[tauri::command]
pub async fn voice_devices() -> AudioDevices {
    tauri::async_runtime::spawn_blocking(crate::audio::cpal_backend::list_devices)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "listing audio devices failed");
            AudioDevices {
                inputs: Vec::new(),
                outputs: Vec::new(),
                default_input: None,
            }
        })
}

/// The window calls this once at start-up, with a channel: a turn that starts from voice (push to
/// talk now, the wake word later) streams its reply there, because it has no `submit_text` call to
/// carry one. Calling it again replaces the channel (a reloaded window). Until it has been called,
/// voice input refuses to start, so a reply never streams to nowhere.
#[tauri::command]
pub fn voice_attach(
    stream: tauri::State<'_, Arc<crate::ipc::voice_stream::VoiceStream>>,
    on_event: Channel<TurnStreamEvent>,
) {
    stream.attach_channel(on_event);
}

/// Push to talk, key down: starts listening. Refused until the window has attached its channel
/// (`voice_attach`), so a reply never streams to nowhere. A start while already listening does
/// nothing and says `Ok`. `async` so it runs inside the Tokio runtime: the reply is streamed there.
#[tauri::command]
pub async fn voice_capture_start(
    stream: tauri::State<'_, Arc<crate::ipc::voice_stream::VoiceStream>>,
    voice: tauri::State<'_, Arc<crate::voice::controller::VoiceController>>,
) -> Result<(), JarvisError> {
    if !stream.is_attached() {
        return Err(JarvisError {
            code: ErrorCode::Internal,
            message: "Voice input is not ready yet.".into(),
            retryable: true,
            request_id: None,
        });
    }
    voice.inner().start_capture()
}

/// Push to talk, key up: use what was said. Safe to call when nothing is capturing.
#[tauri::command]
pub fn voice_capture_stop(voice: tauri::State<'_, Arc<crate::voice::controller::VoiceController>>) {
    voice.stop_capture()
}

/// Downloads the voice models that are not installed yet. Progress arrives as `voice.models_progress`;
/// the answer comes when every file has been downloaded and verified (a checksum for the archive and
/// one for each file). A download that was interrupted resumes where it stopped. One download at a time.
#[tauri::command]
pub async fn voice_models_install(
    app: tauri::AppHandle,
    svc: tauri::State<'_, Arc<Services>>,
    models: tauri::State<'_, Arc<crate::voice::models::ModelManager>>,
) -> Result<VoiceStatus, JarvisError> {
    let for_events = app.clone();
    models
        .install_default(&move |p| publish_event(&for_events, EventKind::VoiceModelsProgress, p))
        .await
        .map_err(model_error)?;
    Ok(VoiceStatus {
        models_ready: models.ready(),
        ..svc.voice_status()
    })
}

/// What the person is told when a model download fails. Plain words, never an address or a path.
fn model_error(e: crate::voice::models::ModelError) -> JarvisError {
    use crate::voice::models::ModelError as M;
    let (code, message, retryable) = match e {
        M::Offline => (
            ErrorCode::NetworkOffline,
            "The speech models could not be downloaded. Check your internet connection and try again; the download continues where it stopped.",
            true,
        ),
        M::Stalled => (
            ErrorCode::NetworkOffline,
            "The download stopped answering. Try again; it continues where it stopped.",
            true,
        ),
        M::Http(_) => (
            ErrorCode::Internal,
            "The download server answered with an error. Try again in a while.",
            true,
        ),
        M::ChecksumMismatch => (
            ErrorCode::Internal,
            "The downloaded file was not the expected one and was thrown away. Try again.",
            true,
        ),
        M::TooBig | M::BadArchive(_) => (
            ErrorCode::Internal,
            "The downloaded file was not what this version of JARVIS expects, so nothing was installed.",
            false,
        ),
        M::Io(_) => (
            ErrorCode::Internal,
            "A file could not be saved. Check that there is free disk space and try again.",
            true,
        ),
        M::Cancelled => (ErrorCode::Internal, "The download was cancelled.", true),
        M::Busy => (
            ErrorCode::Internal,
            "The speech models are already being downloaded.",
            false,
        ),
    };
    if !matches!(e, M::Cancelled | M::Busy) {
        tracing::warn!(error = %e, "the speech model download failed");
    }
    JarvisError {
        code,
        message: message.into(),
        retryable,
        request_id: None,
    }
}

/// Apps that can be put on the allow-list (`AllowedApp.id` is what is checked and launched).
#[tauri::command]
pub fn apps_installed(svc: tauri::State<'_, Arc<Services>>) -> Vec<AllowedApp> {
    svc.apps_installed()
}

#[tauri::command]
pub fn usage_get(svc: tauri::State<'_, Arc<Services>>) -> UsageSummary {
    svc.usage(today())
}

/// Opens the OS folder picker and returns the chosen folder, or `None` if the user
/// cancelled. Nothing is allowed by picking: the window adds the path with
/// `settings_set`, which validates it like a typed one.
#[tauri::command]
pub async fn folder_pick(app: tauri::AppHandle) -> Result<Option<String>, JarvisError> {
    let Some(path) =
        crate::platform::dialog::pick_folder(&app, "Choose a folder JARVIS may use").await
    else {
        return Ok(None);
    };
    crate::platform::dialog::as_setting(&path)
        .map(Some)
        .ok_or_else(|| JarvisError {
            code: ErrorCode::Internal,
            message: "That folder's name contains characters JARVIS cannot store. Rename it or choose another folder.".into(),
            retryable: false,
            request_id: None,
        })
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

/// What a store error looks like to the webview. Database details go to the log only.
fn store_error(e: StoreError) -> JarvisError {
    let (code, message) = match e {
        StoreError::NotFound => (ErrorCode::NotFound, e.to_string()),
        StoreError::Invalid(m) => (ErrorCode::Internal, m),
        StoreError::Db(detail) => {
            tracing::error!(detail = %detail, "memory database error");
            (
                ErrorCode::Internal,
                "The memory database failed.".to_owned(),
            )
        }
    };
    JarvisError {
        code,
        message,
        retryable: false,
        request_id: None,
    }
}

/// Everything the app keeps about the user as one JSON document (memories and history).
/// The webview saves it as a file; the backend never writes it anywhere.
#[tauri::command]
pub fn memory_export(store: tauri::State<'_, Arc<MemoryStore>>) -> Result<String, JarvisError> {
    store.export_json(now_ms()).map_err(store_error)
}

/// Conversations, newest first; `before` is a conversation id.
#[tauri::command]
pub fn history_list(
    store: tauri::State<'_, Arc<MemoryStore>>,
    limit: usize,
    before: Option<String>,
) -> Result<Vec<ConversationSummary>, JarvisError> {
    store
        .list_conversations(limit, before.as_deref())
        .map_err(store_error)
}

#[tauri::command]
pub fn history_get(
    store: tauri::State<'_, Arc<MemoryStore>>,
    conversation_id: String,
) -> Result<Vec<HistoryMessage>, JarvisError> {
    store
        .get_conversation(&conversation_id)
        .map_err(store_error)
}

/// Deletes all history for good. Memories and settings stay.
#[tauri::command]
pub fn history_delete_all(
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<MemoryStore>>,
    runner: tauri::State<'_, Arc<TurnRunner>>,
) -> Result<(), JarvisError> {
    // A reply being written may rest on content that was just deleted, or on untrusted
    // content whose warning flag is about to be cleared: stop it first.
    let sink = AppSink { app, channel: None };
    runner.cancel_active(InterruptReason::UserCancel, &sink);
    store.delete_all_history().map_err(store_error)?;
    // What the next model call is sent comes from memory too; it goes as well.
    runner.forget_conversation();
    Ok(())
}

#[tauri::command]
pub fn history_settings_get(
    store: tauri::State<'_, Arc<MemoryStore>>,
) -> Result<HistorySettings, JarvisError> {
    Ok(HistorySettings {
        retention: store.history_retention().map_err(store_error)?,
    })
}

/// Saves the retention choice; a shorter period removes older history at once.
#[tauri::command]
pub fn history_settings_set(
    store: tauri::State<'_, Arc<MemoryStore>>,
    settings: HistorySettings,
) -> Result<HistorySettings, JarvisError> {
    store
        .set_history_retention(settings.retention, now_ms())
        .map_err(store_error)?;
    Ok(settings)
}

/// Memories, newest change first; optionally only one status.
#[tauri::command]
pub fn memory_list(
    store: tauri::State<'_, Arc<MemoryStore>>,
    status: Option<MemoryStatus>,
) -> Result<Vec<Memory>, JarvisError> {
    store.list(status).map_err(store_error)
}

/// The user accepted a proposed memory: it becomes active.
#[tauri::command]
pub fn memory_accept(
    store: tauri::State<'_, Arc<MemoryStore>>,
    id: String,
) -> Result<Memory, JarvisError> {
    store.accept(&id, now_ms()).map_err(store_error)
}

#[tauri::command]
pub fn memory_edit(
    store: tauri::State<'_, Arc<MemoryStore>>,
    id: String,
    content: String,
) -> Result<Memory, JarvisError> {
    store.edit(&id, &content, now_ms()).map_err(store_error)
}

/// Deletes a memory for good. Denying a proposal is this call.
#[tauri::command]
pub fn memory_delete(
    store: tauri::State<'_, Arc<MemoryStore>>,
    id: String,
) -> Result<(), JarvisError> {
    store.delete(&id).map_err(store_error)
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
