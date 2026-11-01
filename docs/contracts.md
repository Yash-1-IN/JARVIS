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
| `resolve_approval` | `ApprovalDecision` | `void` | M4 |
| `api_key_set` | `key: string` | `ApiKeyStatus` | M2 |
| `api_key_clear` | none | `ApiKeyStatus` | M2 |
| `settings_get` | none | `Settings` | M2 |
| `settings_set` | `patch: SettingsPatch` | `Settings` | M2 |
| `usage_get` | none | `UsageSummary` | M2 |

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

- `turn.interrupted` reasons: `user_cancel`, `hotkey`, `wake_word`, `error`. Afterwards the phase is `listening` for `wake_word` and `idle` otherwise. The turn channel still ends with exactly one `done { stopReason: "cancelled" }`.
- `usage.updated` is emitted after every recorded turn, including cancelled turns (whose tokens are estimated).

## Rules
- The frontend never transitions assistant state; it renders snapshots and sends commands.
- `interrupted`/`cancelled` are events, not states.
- Additive changes keep `PROTOCOL_VERSION`; breaking changes bump it.
