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
