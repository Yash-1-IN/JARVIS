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

Errors are `JarvisError` (`code` from the closed `ErrorCode` enum).
The command signatures marked M2/M4 are agreed here and implemented in their milestone.

## Channel: per-turn stream

`submit_text` takes a Channel and sends `TurnStreamEvent` items in order:
`transcript`, `text_delta`, `tool_proposed`, then exactly one `done` with a `StopReason`.
JSON shape: `{ "type": "text_delta", "text": "..." }`, `{ "type": "done", "stopReason": "end" }`.

## Events (broadcast, low rate)

Every event is an `Envelope<P>`: `{ v, seq, id, ts, kind, payload }`.

| `kind` | `payload` |
| --- | --- |
| `state.snapshot` | `Snapshot` (frontend replaces its copy only if `seq` is newer) |
| `activity.appended` | defined in M4 |
| `approval.requested` | `ApprovalRequest` |
| `error.raised` | `JarvisError` |

## Rules
- The frontend never transitions assistant state; it renders snapshots and sends commands.
- `interrupted`/`cancelled` are events, not states.
- Additive changes keep `PROTOCOL_VERSION`; breaking changes bump it.
