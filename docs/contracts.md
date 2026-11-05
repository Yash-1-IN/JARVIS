# IPC contract

Human-readable view of `src-tauri/src/ipc/`. Rust is the source of truth; TS types are
generated into `src/types/generated/` (`pnpm gen:types`). Protocol version: **1**.
Spec: `docs/spec.md` §5.

## Commands (request/response)

| Command | Args | Returns | Status |
| --- | --- | --- | --- |
| `ping` | none | `PingResponse` | implemented |
| `get_snapshot` | none | `Snapshot` | implemented |
| `submit_text` | `text: string`, `onEvent: Channel<TurnStreamEvent>` | `requestId: string` | M2 |
| `cancel` | `requestId: string` | `void` | M2 |
| `set_mute` | `muted: boolean` | `void` | M2 |
| `resolve_approval` | `decision: ApprovalDecision` (call `invoke("resolve_approval", { decision })`) | `void` | M4 |
| `api_key_set` | `key: string` | `ApiKeyStatus` | M2 |
| `api_key_clear` | none | `ApiKeyStatus` | M2 |
| `settings_get` | none | `Settings` | M2 |
| `settings_set` | `patch: SettingsPatch` | `Settings` | M2 |
| `usage_get` | none | `UsageSummary` | M2 |
| `apps_installed` | none | `AllowedApp[]` | M4 (backend now, platform list by Dev B) |
| `folder_pick` | none | `string \| null` (absolute path, or `null` if cancelled) | M4 follow-up (Dev B) |
| `activity_list` | `limit: number` (1 to 200), `before?: string` (an `ActivityEntry.id`) | `ActivityEntry[]` newest first | M4 |
| `memory_list` | `status?: MemoryStatus` | `Memory[]` newest first | implemented |
| `memory_accept` | `id: string` | `Memory` (now `active`) | implemented |
| `memory_edit` | `id: string`, `content: string` (1 to 500 characters) | `Memory` | implemented |
| `memory_delete` | `id: string` | `void` | implemented |
| `memory_export` | none | `string` (JSON of all memories and history) | implemented |
| `history_list` | `limit: number` (1 to 200), `before?: string` (a `ConversationSummary.id`) | `ConversationSummary[]` newest first | implemented |
| `history_get` | `conversationId: string` | `HistoryMessage[]` oldest first | implemented |
| `history_delete_all` | none | `void` | implemented |
| `history_settings_get` | none | `HistorySettings` | implemented |
| `history_settings_set` | `settings: HistorySettings` | `HistorySettings` | implemented |
| `spotify_status` | none | `SpotifyStatus` | implemented (never connected until the sign-in is built) |
| `spotify_set_client_id` | `clientId: string \| null` | `SpotifyStatus` | implemented |
| `spotify_connect` | none | `SpotifyStatus` | implemented (async: returns when the browser sign-in ends, times out after about 3 minutes, or `spotify_disconnect` cancels it) |
| `spotify_disconnect` | none | `SpotifyStatus` | implemented (safe when not connected) |
| `voice_attach` | `onEvent: Channel<TurnStreamEvent>` | `void` | implemented (the window calls it once at start-up) |
| `voice_status` | none | `VoiceStatus` | implemented (saved choices are real; nothing is installed yet) |
| `voice_settings_set` | `patch: VoiceSettingsPatch` | `VoiceStatus` | implemented |
| `voice_devices` | none | `AudioDevices` | implemented (real device names and the default input; empty lists on a computer with no audio hardware) |
| `voice_capture_start` | none | `void` | implemented: starts a push-to-talk capture; refused (`INTERNAL`, retryable) until `voice_attach` has been called; with no speech model installed a capture that hears speech ends with `MODEL_MISSING` |
| `voice_capture_stop` | none | `void` | implemented (safe when nothing is capturing) |
| `voice_models_install` | none | `VoiceStatus` | implemented: downloads and verifies the models that are not installed yet (progress as `voice.models_progress`), answers when done with `modelsReady: true`; one download at a time; an interrupted one resumes; there is no cancel command yet |

Errors are `JarvisError` (`code` from the closed `ErrorCode` enum).
The command signatures marked M2/M4 are agreed here and implemented in their milestone.

### Settings and API key (M2)
- `api_key_set` is the only time the key crosses IPC. Order: trim and validate the format, check it against the provider's models-list endpoint (no tokens, 5 s timeout), and store it only on success.
  A rejected key returns `PROVIDER_AUTH`; being offline returns `NETWORK_OFFLINE`; in both cases nothing is stored.
  The key never appears in any snapshot, event, error, log or `Debug` output. `ApiKeyStatus.hint` is the last 4 characters, only for keys of 16+ characters.
- `storage: "session_only"` means the OS credential store is unavailable and the key is lost when the app quits; the UI shows a warning (ADR-013).
- `Settings.model` must be one of `Settings.availableModels`, the model ids that have a price in `src-tauri/resources/pricing.json` (D-16). An unknown model is rejected by `settings_set`.
- `SettingsPatch`: omit a field to leave it unchanged. For the caps, send `null` to clear the cap and a number (micro-dollars) to set it.
- Money is integer micro-dollars (1e-6 USD). A cap stops new requests once reached; one long turn can overshoot by its own cost.
- Usage counts reset on UTC day and month boundaries (D-15). `dayResetsAt` and `monthResetsAt` are Unix ms; the UI shows them in local time.

### Spotify (music step 2, issue #68)
Proposed by Developer A in the #68 plan, which Developer B's session did not object to; the shapes below are what this PR adds. The first music step (`music.control`, `music.now_playing`, D-22) needs no contract: it uses the operating system's media controls.
- `SpotifyStatus { clientId: string | null, connected: boolean, displayName: string | null }` has exactly these three fields. **It never carries a token, an authorisation code, a refresh token or any other secret**, now or later; a test checks the field count so one cannot be added by accident. Tokens live only in the Rust backend and the OS keychain, like the API key.
- `clientId` is the Client ID of the developer app the person created at developer.spotify.com (needs their Premium; Spotify's February 2026 development-mode rules apply). It is **not a secret**: the sign-in uses the Authorization Code flow with PKCE, so there is no client secret anywhere. It is 32 characters, `0-9` and `a-f`; `spotify_set_client_id` trims it, stores it lower case, and treats `null` or an empty string as "clear it". Anything else is an `INTERNAL` error ("that does not look like a Spotify Client ID ..."; the message never echoes the input), and **nothing changes**. It is stored in the settings file but is **not part of `Settings` or `SettingsPatch`**, so existing code that builds a `Settings` value does not change.
- `spotify_connect` opens the system browser **from Rust** (no webview permission, no `opener` or `http` plugin) and return when the sign-in ends, times out (about 3 minutes) or is cancelled; the window shows a spinner and a Cancel button that calls `spotify_disconnect`. Errors reuse existing codes: `INTERNAL` with a plain message ("Add your Spotify Client ID first." when there is none; "Signing in to Spotify is not built yet." for now) and `NETWORK_OFFLINE` when Spotify cannot be reached. **No new `ErrorCode`**, so the frontend's `ERROR_INFO` table is unaffected.
- `spotify_disconnect` forgets the token, cancels a sign-in in progress, keeps the Client ID, and is safe to call at any time. Setting a different Client ID also drops a sign-in made with the old one.
- No new event and no new capability: the window polls `spotify_status` when its Music settings open and uses the value returned by each command. `PROTOCOL_VERSION` stays 1 (additive only).
- What the future tools may do is limited by the permission gateway, not by this contract: `music.search` (Low, untrusted results), `music.play` (Low), `music.like`, `music.playlist_create` and `music.playlist_add` (Medium, approval card); nothing that deletes.

### Tools, approvals and the activity log (M4)
- **The model only proposes.** Every tool call goes through the permission gateway in Rust (spec §9.3). The webview never calls a tool; it can only answer an approval.
- `approval.requested` carries an `ApprovalRequest` and the same request also appears in `Snapshot.pendingApprovals`. The card shows `summary` and `targets` exactly as the backend wrote them (the model never writes them), and says when `tainted` (untrusted content is in the context) or when the content will be sent to the cloud provider.
- `resolve_approval(ApprovalDecision)` must echo `argsDigest`. It fails with `TOOL_DENIED` when the approval is unknown, already answered, expired (60 s) or the digest does not match; the tool then does not run. Approvals are single-use.
- **Voice never approves a `high` risk action** (`allowVoice` is false), and `resolve_approval` is only reachable from the window, so a spoken "yes" cannot call it.
- `activity.appended` carries one `ActivityEntry` for every tool call, including denied, expired and prohibited ones (`outcome: "not_run"`). `activity_list` pages the same rows newest first. Entries contain what ran, on what and how it ended, **never file contents or arguments**.
- `ToolDecision`: `auto_allowed`, `approved`, `denied`, `expired`, `prohibited`. `ToolOutcome`: `ok`, `failed`, `timed_out`, `cancelled`, `not_run`. `ToolRisk`: `low`, `medium`, `high`.
- **Policy settings:** `Settings.allowedFolders` replaces the whole list when patched; folders are canonicalised and must exist. `Settings.allowedApps` is a list of `AllowedApp { id, name }`: the **id** is a stable platform id (macOS bundle id, Windows AppUserModelID or canonical .exe path, Linux .desktop id) and is the only thing the gateway checks and `apps.launch` starts; the **name** is display only and never used for matching. `settings_set` accepts only ids that `apps_installed` returns, and takes the display name from that list, so a patch cannot rename an app. Until the platform adapter exists, `apps_installed` is empty and an allow-list cannot be set (clearing it always works). `autoAllowMedium` is off by default, never applies to `high`, and never applies while the context is tainted.
- **Folder picker:** `folder_pick` opens the operating system's folder picker (Finder, Explorer, GTK) from Rust and returns the chosen folder's absolute path, or `null` when the user cancels. **Picking allows nothing:** the window adds the path with `settings_set`, which validates and canonicalises it exactly like a typed one. The dialog plugin is used from Rust only; the webview gets no dialog, fs or other plugin permission (capabilities stay `core:default`). A folder whose name is not valid Unicode is refused (`INTERNAL`, message safe to show).
- `Settings` gained three fields (`allowedFolders`, `allowedApps`, `autoAllowMedium`); the wire change is additive, but code that builds a `Settings` value (mocks, tests) must add them.

## Channel: per-turn stream

`submit_text` takes a Channel and sends `TurnStreamEvent` items in order:
`transcript`, `text_delta`, `tool_proposed`, then exactly one `done` with a `StopReason`.
JSON shape: `{ "type": "text_delta", "text": "..." }`, `{ "type": "done", "stopReason": "end" }`.

## Events (broadcast, low rate)

Every event is an `Envelope<P>`: `{ v, seq, id, ts, kind, payload }`, where `kind` is the closed `EventKind` enum.

**Tauri event name:** Tauri event names cannot contain `.`, so the frontend listens on the `kind` with `.` replaced by `_`, for example `listen("state_snapshot")`. The envelope's `kind` field keeps the dotted name. Rust has `EventKind::tauri_event_name()`.

| `kind` | Tauri event | `payload` |
| --- | --- | --- |
| `state.snapshot` | `state_snapshot` | `Snapshot` (frontend replaces its copy only if `seq` is newer) |
| `activity.appended` | `activity_appended` | defined in M4 |
| `approval.requested` | `approval_requested` | `ApprovalRequest` |
| `error.raised` | `error_raised` | `JarvisError` |
| `turn.interrupted` | `turn_interrupted` | `TurnInterrupted { requestId, reason }` |
| `usage.updated` | `usage_updated` | `UsageSummary` |
| `memory.proposed` | `memory_proposed` | `Memory` (status `proposed`) |

- `turn.interrupted` reasons: `user_cancel`, `hotkey`, `wake_word`, `error`. Afterwards the phase is `listening` for `wake_word` and `idle` otherwise. The turn channel still ends with exactly one `done { stopReason: "cancelled" }`.
- `usage.updated` is emitted after every recorded turn, including cancelled turns (whose tokens are estimated).

## Rules
- The frontend never transitions assistant state; it renders snapshots and sends commands.
- `interrupted`/`cancelled` are events, not states.
- Additive changes keep `PROTOCOL_VERSION`; breaking changes bump it.

### Memory and history (M5)
Agreed here before any code (spec §8). All the `memory_*` and `history_*` commands and the `memory.proposed` event are implemented.
- **Nothing becomes a memory without the user.** "Remember that X" and a model's `memory.propose` both create a `proposed` row and emit `memory.proposed`; the UI shows a card with the exact `content`. **Accept** is `memory_accept` (the row becomes `active`). **Deny** is `memory_delete`: a denied proposal is gone, nothing is stored. Only `active` memories are ever put into a prompt, as quoted data, never as instructions.
- `memory_edit` changes the text of any memory. Editing an `active` memory keeps it `active`; the new text is what the user sees and confirms in the form. Maximum 500 characters, counted as Unicode code points (Rust `chars().count()`, not UTF-16 units or bytes); control characters are removed and an empty text is rejected.
- **Delete is a hard delete.** The text is removed from the database, the search index, retrieval and the next prompt (a backend test asserts this).
- `history_*`: conversations end after 10 minutes idle or on "new conversation". Stored messages hold tool results as one-line summaries, never file contents; raw audio, API keys and detected secrets are never stored. `history_delete_all` leaves an empty database. `memory_export` returns one JSON document and never contains API keys.
- `HistorySettings.retention` is `off | days7 | days30 | days365 | forever`; the default is `days30`. Changing it purges what is now older than the limit on the next purge run (at start-up and daily).
- Money, time and ids follow the existing rules (Unix ms, UUIDv7 strings). Errors are `JarvisError`; an invalid text is `INTERNAL` with a message the UI can show. `NOT_FOUND` is now a code: an unknown memory id returns it (message "That memory no longer exists."). `HistoryMessage.role` is the closed enum `user | assistant | tool`.
- Additive only: no existing type changed, so `PROTOCOL_VERSION` stays 1.
- "Remember that X" (said or typed) and the model's `memory.propose` both end in the same place: a `proposed` row and a `memory.proposed` event. The reply to the user does not repeat the text; the card does. For a cloud turn the backend adds up to five **active** memories that match the question to the system prompt as quoted data (JSON strings, labelled as notes and not instructions); proposed and archived memories are never added.
- What is recorded: every user message, the reply (cloud or local), and one-line summaries of tool calls (the same sentence the approval card and the activity log show; never file contents). With retention `off` nothing is recorded. `memory_export` returns `{ format: "jarvis-export", version: 1, exportedAt, historyRetention, memories, history }`; the backend never writes it to disk, the window saves it. Retention is applied when the app starts, once a day, and at once when it is shortened.

### Voice (M3, `docs/m3-plan.md` step 1)
Proposed by Developer A in the M3 plan; Developer B reviews this before anything builds on it. **Additive**: no existing type, command or error code changes, so nothing in `src/**` breaks, and `PROTOCOL_VERSION` stays 1.
- **Nothing here carries audio.** `VoiceStatus` has exactly seven fields: `wakeEnabled`, `followUpWindow`, `voice`, `availableVoices`, `inputDevice`, `micPermission`, `modelsReady`. It never holds a path, a secret or sound; a test checks the field count so one cannot be added by accident. Audio stays in the Rust backend, in RAM, and is never written to disk.
- **Voice choices live outside `Settings`**, as the Spotify Client ID does, so the frontend's `Settings` values (and every mock that builds one) stay as they are. They are saved in the settings file. `VoiceSettingsPatch` follows the `SettingsPatch` rules: omit a field to leave it alone; `inputDevice: null` returns to the system default. The wake word is **off by default**; the follow-up window is on.
- Validation: `voice` is 1 to 64 letters, digits, `_` or `-`; `inputDevice` is 1 to 200 characters with no control characters (an empty string also means the default). A bad value is an `INTERNAL` error with a plain message that never echoes the input, and **nothing in the patch is applied**. Whether a voice is installed, or a microphone exists, is checked where they are known (the backend, later), not by this contract.
- **Commands.** `voice_status` and `voice_settings_set` are real. `voice_devices` lists the real microphones and speakers. `voice_capture_start` is real: the microphone opens, the end of speech is found, and the assistant goes `listening`, `transcribing`, `thinking`; until the speech models are installed it ends with `MODEL_MISSING` ("The speech models are not installed yet."), and `voice_models_install` downloads them (see below); the UI should show the error messages, not hide them. `modelsReady` in `voice_status` is read from the models folder each time and is true only when every file of the default set was downloaded and passed its checksum. A failed download answers `NETWORK_OFFLINE` (retryable: no connection, or the server stopped answering) or `INTERNAL` with a plain message (retryable when trying again can help, such as a wrong download that was thrown away). Muting, Esc or "stop" ends a capture at once, and a capture whose microphone never starts gives up after 20 s with `DEVICE_LOST`. `voice_capture_start` and `voice_capture_stop` are push to talk, key down and key up (stop is also cancel); a wake word starts a capture inside Rust with no command at all.
- **Events.** `transcript.partial` and `transcript.final` carry `Transcript { requestId, text }`, text only. `voice.models_progress` carries `ModelProgress { name, doneBytes, totalBytes }` while models download. Tauri event names replace `.` with `_` (`transcript_final`, `voice_models_progress`) as for the others. There is **no wake event**: a wake changes the phase to `listening` and arrives as a `state.snapshot`, because the orchestrator owns the only state machine.
- **Errors.** Reuse existing codes: `MIC_PERMISSION_DENIED` (shown with guidance and a link to the OS setting), `DEVICE_LOST`, `MODEL_MISSING`, `MODEL_CHECKSUM`. **No new `ErrorCode`**, so the frontend's `ERROR_INFO` table is unaffected. `micPermission` is `granted`, `denied` or `unknown`; every build says `unknown` until capture exists.
- **Which microphone is used.** `AudioDevices.defaultInput` is the name the system default points at now (`null` if none or unknown), so the window can show the input that is really in use when `inputDevice` is `null` and warn about a Bluetooth headset (S1 report). A per-input Bluetooth hint is proposed with step 3 and is additive.
- **Push to talk and a lost key-up.** The backend ends one capture at the 15 second cap (spec §6.2), and a second `voice_capture_start` while a capture is running is a no-op that returns `Ok`. The window should still send `voice_capture_stop` on blur, `visibilitychange` and `pointercancel`.
- **When to speak.** Pressing push to talk moves the phase to `listening` at once, but the microphone has to open first, which can take a noticeable moment (a fraction of a second to more than a second on Windows). `mic` becomes `capturing` only when the device is open and delivering sound, and goes back to what it was when the capture ends. That is the cue to speak: words said before it are lost because nothing was listening. The window should show something like "Starting microphone…" while `phase` is `listening` and `mic` is not yet `capturing`, and "Speak now" after. The backend logs how long the opening took (`voice: microphone open`, `voice: first sound arrived`).
- **Limitation: device names are not unique.** Two identical microphones, or a name that changes after a driver update, make `inputDevice` ambiguous. Acceptable for the alpha: if the saved name is not found, the backend falls back to the default and the window shows `defaultInput`.
- **Where a voice turn streams (`voice_attach`).** A typed turn streams its reply on the channel the window passes to `submit_text`. A voice turn starts from Rust, so the window calls `voice_attach(onEvent)` **once at start-up** and voice-started turns stream on that channel (the same `TurnStreamEvent`s, ending with exactly one `done`). Calling it again **replaces** the channel, so a reloaded window works; a channel whose window has closed is dropped. **`voice_capture_start` answers an `INTERNAL` error ("Voice is not ready: the window has not attached.") until `voice_attach` has been called**, so a reply never streams to nowhere. Phase, microphone and heard-text changes still arrive as `state.snapshot` and `transcript.final` events, which need no channel. A typed turn keeps using its own channel; the attached one carries only voice-started turns. No new type, no new event, and `PROTOCOL_VERSION` stays 1.
- **One turn at a time, and a reload in the middle.** At most one turn is active at a time, typed or voice, so the events arriving on the attached channel belong to one turn at a time: `transcript.final { requestId, text }` opens it (the heard words) and the events that follow on the attached channel belong to it until its `done`. A voice capture started while a typed turn is running is **refused** (`INTERNAL`, "JARVIS is busy with another request."), and a typed message sent while a voice turn is listening, transcribing or running is refused the same way. "Stop", "mute" and "unmute" are controls, not turns, and still work. If the window **reloads in the middle of a voice turn**, the old channel is dropped and the rest of that turn's events, including its `done`, are not delivered to the new window: the window must trust `state.snapshot` (the phase returns to idle) and not wait for a `done`.
- Not in this PR: a microphone level stream for a level meter. It would be a Tauri Channel (a stream, not an event) and is proposed for step 8 with the Voice UI.
