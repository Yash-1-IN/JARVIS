# JARVIS — Revised Engineering Specification v1.0

Oct 31, 2026 · @Dharmendra Singh

## 1. Verdict

The brief's principles are right, but it is not yet implementable: it has two competing state machines, no plan for the microphone hearing JARVIS's own voice, licence-blocked defaults for the wake word and voice, and an order that leaves the riskiest work until month two. This revision keeps every defining goal and makes the changes below.

Tags used throughout: **\[Verified\]** = checked against a source on 31 Oct 2026 (see Sources); **\[Rec\]** = architect's recommendation; **\[Open\]** = needs an experiment before it is decided.

| # | Change | Why it matters |
| --- | --- | --- |
| 1 | One native speech engine: the official `sherpa-onnx` Rust crate for wake word, VAD, STT and TTS | One toolchain to build, package and sign instead of three; Apache-2.0 \[Verified\] |
| 2 | Default voice moves from Piper to Kokoro-82M (`bm_george` / `bm_lewis`) with a GPL-free phonemizer | Piper's maintained repo is now GPL-3.0-or-later; Kokoro is Apache-2.0 \[Verified\] |
| 3 | The ready-made openWakeWord "hey jarvis" model is personal-use only | Its models are CC BY-NC-SA 4.0 \[Verified\]; distribution intent must be decided in M0 |
| 4 | Split `JarvisState` into `MicState` and `AssistantPhase`; add `awaiting_approval`; make `interrupted` an event | Muted and speaking can be true at once; one enum cannot express it |
| 5 | The M1 mock is a `MockTransport` that emits the real contract, not a frontend state machine | Removes the split-brain the brief itself forbids |
| 6 | Typed contract generated from Rust; streams use Tauri Channels, not events | Tauri events are untyped JSON and not built for throughput \[Verified\] |
| 7 | Barge-in needs echo handling; alpha ships button/hotkey/wake-word interrupt, full AEC is a spike | Otherwise the mic transcribes JARVIS talking |
| 8 | High-risk approvals are on-screen only, never by voice | A TV or JARVIS's own speaker could say "yes" |
| 9 | Taint rule: once file or web content enters the context, every medium/high action asks | The concrete defence against prompt injection |
| 10 | Offline mode = a fixed set of local intents, no local LLM in alpha | Makes "useful without cloud" testable on an average laptop |
| 11 | Voice spikes start in M0/M1, text chat (Brain) ships before full voice | Dev A is idle-ish while Dev B builds UI; voice is the biggest unknown |
| 12 | Tools and permissions before memory; rebalance work toward Developer B | Memory consent reuses the approval card; Dev A owned \~80% of the brief |
| 13 | 2D hologram fallback is mandatory and is the Linux default until measured | WebGL on WebKitGTK can silently fall to a slow path \[Verified\] |
| 14 | macOS builds are ad-hoc signed, not notarised; cloud sessions do code and tests, hardware testing stays local | No Apple Developer membership is available, and cloud VMs have no mic, display, macOS or Windows (§6.9, §18.1) |

## 2. Challenges to the original brief

Thirty-five issues, grouped by kind; each row's resolution is specified later in this document.

### 2.1 Contradictions

| Issue | Where in the brief | Resolution |
| --- | --- | --- |
| "Frontend must reflect backend state" vs "build a mock assistant state machine" in the frontend | §6 vs §11 Dev B prompt | `MockTransport` emits real contract events; the store is a pure projection (§5) |
| `muted` is a value of the same enum as `speaking` | §6 | Two axes: `MicState` and `AssistantPhase` (§5) |
| `interrupted` modelled as a state | §6 | It is an event; the phase returns to `listening` or `idle` |
| "Remain useful when cloud is unavailable" but all reasoning is cloud | §2.1, §3.3 | Defined offline intent set (§3.3) |
| "Interruptible while speaking" but no echo handling | §3.1 | Interrupt paths + AEC spike (§6.6) |
| "Audio failures must not crash the app" but native C++ inference runs in-process | §9 | A segfault in FFI code cannot be caught; in-process for alpha with a soak-test trigger to move to a sidecar (§4.3) |
| "Keep dependencies minimal" but three native speech stacks (sherpa-onnx, whisper.cpp, Piper) | §3.2 | One engine (ADR-004) |
| "Do not store raw recordings" but wake-word tuning needs audio | §3.1, §3.2 | Opt-in, local-only test corpus recorded with a separate dev tool, never by the app |
| Persona named JARVIS with Iron Man styling, distribution undecided | §1, §2.2 | Fine for personal use; a public or commercial release needs a name and IP check (\[Open\] D-14) |

### 2.2 Missing requirements

- **Distribution intent** (personal, open source, commercial). It decides which wake-word, TTS and phonemizer licences are usable.
- **Window model**: tray icon, compact always-on-top orb, start at login, show/hide hotkey.
- **Push-to-talk** as a fallback when wake word fails or is disabled.
- **Echo cancellation / self-hearing** during TTS playback.
- **Follow-up window**: listening for a short time after a reply without a second wake word.
- **Conversation lifecycle**: when a session ends, how much history is sent, token budget.
- **Language and accent scope**: English only for alpha; test with the developers' own accents.
- **Model download UX**: where models live, size budget, checksum, resumable download, offline install.
- **Rendering model output safely**: the webview has IPC access, so markdown from the model is an XSS path.
- **OS microphone permission**: macOS needs `NSMicrophoneUsageDescription` or capture silently returns zeros \[Verified\]; Windows has a privacy toggle that must be detected.
- **Runtime API spend cap** and usage display.
- **History retention default** and "delete everything".
- **Settings schema versioning and migration**.
- **Telemetry and crash-report policy**: none leaves the machine by default.
- **Recorded audio test fixtures** for repeatable voice tests.

### 2.3 Unrealistic assumptions

- **"Hey JARVIS works on all OSes"**: the engine is OS-neutral; microphones, rooms and accents are not. Success is a measured false-accept rate, not a demo.
- **$200 covers delivery**: runtime API tokens are separate, and without Apple Developer membership macOS builds cannot be notarised.
- **Hologram + always-on wake word + local STT/TTS on an average laptop** without budgets: requires lazy model loading and render-on-demand (§10).
- **Playwright "when needed"**: it needs a Node runtime and browser binaries of hundreds of MB; it is post-alpha.
- **Whisper for low latency**: Whisper is non-streaming; fine for short commands, but measure against Moonshine on the reference laptop.
- **Linux parity**: global hotkeys do not work under Wayland with Tauri's plugin \[Verified\]; WebGL can silently run slowly \[Verified\].

### 2.4 Overengineering to cut

- Empty module folders for every future milestone: create a folder when its milestone starts.
- Multi-provider abstraction before a second provider exists: one trait, one implementation.
- Zod validation of events from our own backend when types are generated: keep Zod for user-supplied data only.
- Activity and approval UI built in M1 before tools exist: design and stub it, wire it in M4.
- A Rust SDK dependency for Anthropic: Rust SDKs are unofficial \[Verified\]; a 300-line SSE client is smaller and owned.

## 3. Product scope: the first usable alpha

Alpha 0.1 is done when a person on Windows, macOS or Linux (X11) can say "Hey JARVIS, what's my battery?", "open Spotify" or ask a general question, hear a British voice answer within the §10 budgets, interrupt it, and see and approve every action it takes.

### 3.1 In scope for alpha

| Area | Alpha capability |
| --- | --- |
| Input | Wake word, push-to-talk hotkey (X11/Windows/macOS), text box |
| Voice | Local wake word, VAD, STT, TTS; one British male voice; interrupt by button, Esc, hotkey or wake word |
| Brain | One cloud provider (Anthropic), streaming text, tool calls, spend cap |
| Offline | Local intents in §3.3 |
| Tools | System status, launch allow-listed apps, search/read/move/rename inside allowed folders (§9.4) |
| Memory | History with retention setting; durable memories only with user confirmation; view/edit/delete |
| UI | Main window, compact orb mode, tray, activity log, approval cards, settings, 3D and 2D hologram |
| Packaging | Linux .deb/AppImage; unsigned Windows installer; macOS ad-hoc signed, not notarised (§6.9) |

### 3.2 Explicitly out of scope for alpha

- Deleting files, running scripts, installing software, sending messages, purchases, changing system settings.
- Browser automation (Playwright) and reading other apps' windows.
- Local LLM; second cloud provider; embeddings / vector search.
- Languages other than English; voice cloning; multiple voices.
- Wayland global hotkey (documented limitation; text and wake word still work).
- Mobile, sync, multi-user, auto-update of the app (model updates are in scope).

### 3.3 Offline mode

Without network, JARVIS says it is offline once, then still handles these through a deterministic local intent router (no LLM):

| Intent | Example | Works offline |
| --- | --- | --- |
| Time and date | "What time is it?" | Yes |
| System status | "How's my battery?" | Yes |
| Launch allow-listed app | "Open Spotify" | Yes |
| Mute / unmute / stop | "Stop", "Go quiet" | Yes |
| Repeat last answer | "Say that again" | Yes |
| What can you do offline | "What can you do?" | Yes |
| File search in allowed folders | "Find my tax PDF" | Yes, by filename pattern only |
| General questions, multi-step tasks | anything else | No: says so and keeps the transcript for retry |

The same router runs first when online, so these intents never pay cloud latency or cost.

## 4. Architecture

One Tauri process with a single authoritative orchestrator in Rust; the webview renders state and sends intents, and nothing the model or the webview says can reach a tool except through the permission gateway.

### 4.1 Module boundaries

| Module (Rust unless noted) | Owns | Must never | Owner |
| --- | --- | --- | --- |
| `orchestrator` | The state machine, turn lifecycle, request IDs, cancellation tree | Call platform APIs directly | A |
| `audio` | Device enumeration, capture, playback, ring buffers, device-change recovery | Allocate or lock inside the audio callback | A |
| `voice` | Wake word, VAD, STT, TTS behind traits; model loading/unloading | Persist audio; talk to the network | A |
| `intelligence` | `ModelProvider` trait, Anthropic client, context builder, local intent router, spend meter | Execute tools | A |
| `tools` | Tool registry, schemas, tool implementations | Run without a gateway ticket | A (B for `system.*`, `apps.*`) |
| `permissions` | Policy, risk evaluation, approvals, taint flag, audit log | Be configurable by the model | A |
| `memory` | SQLite, migrations, repositories, retrieval | Store raw audio or secrets | A |
| `platform` | Per-OS adapters: app launch, system info, paths, keychain, mic permission | Contain business logic | B (with A review) |
| `ipc` | Commands, Channels, events, generated TS types, protocol version | Expose SQL, fs or shell to the webview | Shared |
| `models` | Model manifest, download, checksum, storage | Load an unverified file | B |
| Frontend `transport/` (TS) | `TauriTransport` and `MockTransport` with one interface | Hold assistant logic | B |
| Frontend `stores/` (TS) | Zustand projection of backend snapshots + view-only UI state | Transition assistant state on its own | B |

### 4.2 Threads and concurrency rules

1. **Audio thread** (cpal callback): copies samples into a lock-free ring buffer and returns. Nothing else.
2. **Voice worker thread** (dedicated `std::thread`): wake word and VAD run continuously; STT and TTS run on demand. Inference never runs on Tokio worker threads.
3. **Tokio runtime**: orchestrator, network, database (via `spawn_blocking`), tools.
4. **Main thread**: Tauri/webview only.
5. Every long operation receives a `CancellationToken` child of its turn's token; cancelling a turn cancels STT, the HTTP stream, TTS and any tool still running.
6. All cross-module queues are bounded; a full queue drops audio-level updates, never commands.

### 4.3 Process isolation decision

&#91;Rec\] Run inference in-process for alpha, but keep the `voice` module's public interface message-based (commands in, events out) so it can move to a sidecar binary without changing callers. **Trigger to move**: any native crash during the M3 8-hour soak test or the alpha fault-injection run. A sidecar costs a Cargo workspace, a second signed binary and an IPC protocol; do not pay that cost speculatively.

### 4.4 Data flow

The diagram below replaces the brief's linear flow. The two changes that matter: the **local intent router** sits before the cloud model, and **tool results return to the model marked untrusted**, setting the taint flag.

&#91;embedded content: alpha data flow · 12 components, 2 trust anchors\]

Local intents and model proposals both enter the gateway; results return to the model as untrusted data and set the taint flag.

## 5. State model and frontend/backend contracts

The backend owns one state machine and publishes a full snapshot with a sequence number on every change; the frontend store only ever replaces its copy with a newer snapshot.

### 5.1 Two state axes

| Axis | Values | Notes |
| --- | --- | --- |
| `MicState` | `off`, `muted`, `standby`, `capturing`, `unavailable` | `standby` = only wake word/VAD running locally; `unavailable` = no device or OS permission denied |
| `AssistantPhase` | `idle`, `listening`, `transcribing`, `thinking`, `awaiting_approval`, `acting`, `speaking`, `error` | `listening` = capturing a request after wake/PTT; `acting` = a tool is running |

`interrupted` and `cancelled` are **events**, emitted with the request ID; afterwards the phase becomes `listening` (if the interrupt was the wake word) or `idle`. The state diagram at the end of this section shows legal transitions; any other transition is a bug and is logged.

### 5.2 Transport rules

| Mechanism | Use for | Examples |
| --- | --- | --- |
| Command (request/response) | Anything the UI asks for | `ping`, `get_snapshot`, `submit_text`, `cancel`, `set_mute`, `resolve_approval`, `settings_get/set`, `memory_*`, `models_status` |
| Channel (per command, ordered) | High-rate or long streams | assistant token stream for one turn; audio level meter (≤ 30 Hz); model download progress |
| Event (broadcast) | Low-rate state changes | `state.snapshot`, `activity.appended`, `approval.requested`, `error.raised` |

Tauri documents events as not designed for low latency or high throughput, with JSON payloads and no strong typing; Channels are the fast, ordered path \[Verified\].

### 5.3 Conventions

- **Envelope** on every event: `{ v, seq, id, ts, kind, payload }`; `v` = `PROTOCOL_VERSION` (integer, starts at 1). The frontend shows a blocking error on mismatch.
- **IDs**: UUIDv7 strings (sortable). Every user turn has a `requestId`; `cancel(requestId)` cancels the whole tree.
- **Timestamps**: Unix milliseconds as `number`.
- **Errors**: `{ code, message, retryable, requestId? }` with `code` from a closed enum: `MIC_PERMISSION_DENIED`, `DEVICE_LOST`, `MODEL_MISSING`, `MODEL_CHECKSUM`, `PROVIDER_AUTH`, `PROVIDER_RATE_LIMIT`, `PROVIDER_UNAVAILABLE`, `NETWORK_OFFLINE`, `SPEND_CAP_REACHED`, `TOOL_DENIED`, `TOOL_TIMEOUT`, `TOOL_FAILED`, `INTERNAL`.
- **Type generation**: Rust is the source of truth; TS types are generated into `src/types/generated/` and CI fails if they drift. \[Rec\] use a stable generator such as `ts-rs` plus \~10 hand-written invoke wrappers; `tauri-specta` is attractive but still a release candidate pinned to an RC of `specta` \[Verified\], so adopt it only with exact version pins (D-07).

### 5.4 Revised contract (TypeScript view of the generated types)

```typescript
export const PROTOCOL_VERSION = 1;

export type MicState = "off" | "muted" | "standby" | "capturing" | "unavailable";
export type AssistantPhase =
  | "idle" | "listening" | "transcribing" | "thinking"
  | "awaiting_approval" | "acting" | "speaking" | "error";

export interface Snapshot {
  seq: number;
  mic: MicState;
  phase: AssistantPhase;
  activeRequestId: string | null;
  network: "online" | "offline" | "unknown";
  cloudInUse: boolean;            // true while data is leaving the machine
  pendingApprovals: ApprovalRequest[];
  lastError: JarvisError | null;
}

export interface Envelope<K extends string, P> {
  v: number; seq: number; id: string; ts: number; kind: K; payload: P;
}

export type TurnStreamEvent =
  | { type: "transcript"; text: string; final: boolean }
  | { type: "text_delta"; text: string }
  | { type: "tool_proposed"; toolCallId: string; tool: string }
  | { type: "done"; stopReason: "end" | "cancelled" | "error" | "spend_cap" };

export interface ApprovalRequest {
  approvalId: string;
  requestId: string;
  tool: string;
  risk: "medium" | "high";
  summary: string;          // human sentence, generated by the backend, not the model
  targets: string[];        // exact paths / app IDs
  argsDigest: string;       // SHA-256 of canonical args; approval binds to it
  expiresAt: number;        // ts + 60 s
  allowVoice: boolean;      // always false for high risk
  tainted: boolean;         // untrusted content is in context
}

export interface ApprovalDecision {
  approvalId: string; argsDigest: string; decision: "approve" | "deny";
}

export interface JarvisError {
  code: ErrorCode; message: string; retryable: boolean; requestId?: string;
}
```

### 5.5 The mock

`MockTransport` implements the same interface as `TauriTransport`, replays scripted envelopes from `src/transport/mock/scripts/*.json`, and renders a persistent **MOCK** badge. It is selected only by `VITE_TRANSPORT=mock` in dev builds; release builds exclude it at compile time.

&#91;embedded content: AssistantPhase state machine · 7 phases plus error\]

Every tool call leaves `thinking` through either `acting` (low risk) or `awaiting_approval`, and every result returns to `thinking` so the model sees it.

## 6. Voice pipeline

Use the official `sherpa-onnx` Rust crate as the single speech engine, `cpal` for audio I/O, and treat the wake word and the voice as experiments with pass/fail criteria, not assumptions.

### 6.1 Engine \[Verified\]

The official `sherpa-onnx` crate (v2.0.1, 27 Oct 2026, Apache-2.0) wraps the C API, links statically by default and downloads prebuilt libraries for Windows, macOS and Linux on x64 and arm64. One crate covers keyword spotting, Silero and TEN VAD, offline ASR (Whisper, Moonshine, Parakeet, Zipformer and others), streaming ASR, and TTS (Kokoro, Kitten, VITS/Piper-format, Matcha). The community `sherpa-rs` crate is deprecated in favour of it.

### 6.2 Stages

1. **Capture**: `cpal` input at the device rate → resample to 16 kHz mono → ring buffer.
2. **Pre-roll**: the last 2 s of audio stay in RAM so the start of the request is not lost; overwritten continuously, never written to disk, discarded after use.
3. **Wake word**: runs continuously on the voice thread while `MicState = standby`.
4. **VAD**: after wake or push-to-talk, end of speech = 700 ms of silence (tunable), hard cap 15 s per request.
5. **STT**: offline model on the captured segment.
6. **Router**: local intent match first, otherwise cloud (§7).
7. **TTS**: synthesise sentence by sentence while the model is still streaming; first sentence plays as soon as it is ready.
8. **Follow-up window**: after speaking, listen for 6 s without a wake word, with a visible indicator; off switch in settings.

### 6.3 Wake word candidates \[Open, D-02\]

| Candidate | Licence | Strength | Risk |
| --- | --- | --- | --- |
| sherpa-onnx open-vocabulary KWS (zipformer, \~3M params) | Apache-2.0 engine; check each model card | Any phrase, no training; same engine as the rest | Accuracy for "hey jarvis" unknown |
| openWakeWord pretrained "hey jarvis" | CC BY-NC-SA 4.0 \[Verified\] | Purpose-built for this phrase | Non-commercial only; needs a separate ONNX runner |
| Custom model trained on synthetic speech (openWakeWord or wakeforge pipeline) | Depends on training data and TTS used | Licence under our control | 1–3 days of ML work; quality varies |
| Commercial SDK | Proprietary | Mature tooling | Cost, closed source, terms to check |

The spike in §15 decides. Until then, push-to-talk is the guaranteed input path.

### 6.4 Speech-to-text \[Open, D-04\]

Compare, through the same crate, **Moonshine base** (English models MIT \[Verified\]) and **Whisper base.en / small.en** on the team's own recorded corpus. Choose the smallest model that meets the WER and latency budgets in §10.

### 6.5 Text-to-speech \[Open, D-03\]

- **Default candidate: Kokoro-82M**, Apache-2.0, with British male voicepacks `bm_george` and `bm_lewis` \[Verified\].
- **Phonemizer licence trap**: espeak-ng and `piper-phonemize` are GPL-3.0 \[Verified\]. Use sherpa-onnx's lexicon mode or an MIT G2P such as `misaki-rs`, and record the choice in ADR-007.
- **Piper**: the maintained repo `OHF-Voice/piper1-gpl` is GPL-3.0-or-later; the MIT `rhasspy/piper` repo was archived on 6 Oct 2025, and each voice has its own licence \[Verified\]. Keep Piper voices as a fallback only if the licence review passes.
- Never clone or imitate a real actor's voice; the persona is "refined British", not a specific person.

### 6.6 Interruption and echo \[Open, D-05\]

Without echo cancellation the microphone hears JARVIS speaking. Alpha ships four interrupt paths that do not need AEC: the stop button, Esc in the focused window, the global hotkey, and the wake word during speech with a raised threshold while TTS is playing. A spike evaluates `sonora` (pure-Rust port of WebRTC AEC3, BSD-3-Clause, v0.1.0, young) \[Verified\] against OS voice-processing modes. Headphones remove the problem and are recommended in the setup guide.

### 6.7 Platform specifics

| OS | Requirement |
| --- | --- |
| macOS | `src-tauri/Info.plist` with `NSMicrophoneUsageDescription`, otherwise capture silently returns zeros \[Verified\]; any build with hardened runtime, ad-hoc signed included, needs `com.apple.security.device.audio-input` \[Verified\]; dev builds attach the permission to the terminal app |
| Windows | Detect the "Let apps access your microphone" privacy setting and show `MIC_PERMISSION_DENIED` with a link to settings |
| Linux | PipeWire/PulseAudio through ALSA; test on one X11 and one Wayland desktop |
| All | Device hot-plug: on loss, phase → `error`, mic → `unavailable`, retry every 2 s, recover without restart |

### 6.8 Model management

Models are downloaded on first run, not bundled. `models/manifest.json` lists for each file: URL, size, SHA-256, licence, and purpose. Downloads are resumable, verified before use, and stored in the OS app-data directory. An "import from folder" option supports offline installs. Default set budget: ≤ 600 MB.

### 6.9 macOS without an Apple Developer account

Without a membership there is no Developer ID certificate and no notarisation, but the Mac developer can still build, run and test everything; only distribution to other Macs gets harder.

| Situation | What happens | What to do |
| --- | --- | --- |
| Development (`pnpm tauri dev`) | Runs normally; macOS grants the mic to the terminal app | Nothing extra |
| Release build on the same Mac | Tauri can ad-hoc sign with `signingIdentity: "-"`; Apple Silicon requires a signature for apps from the internet \[Verified\] | Set `"signingIdentity": "-"` in `tauri.conf.json` so the `.app` is signed before the DMG is made |
| Mic permission after rebuilds | An ad-hoc signature changes with every build, so macOS may ask for the mic again \[Rec: confirm in M3\] | Optional: create a self-signed code-signing certificate in Keychain Access and use its name as `signingIdentity` for a stable local identity |
| Another Mac opens the download | Gatekeeper blocks it; since macOS 15 Sequoia, Control-click no longer bypasses it, and the user approves it in System Settings → Privacy & Security → Open Anyway \[Verified\] | Document the steps with screenshots in `docs/setup-macos.md`; developer testers can build from source instead |
| Hardened runtime on | Entitlements are enforced | Keep `Entitlements.plist` with `com.apple.security.device.audio-input`, plus `NSMicrophoneUsageDescription`, in every build |

Consequence: the macOS alpha audience is the team and technically comfortable testers. A public or commercial macOS release needs a paid membership later (D-01, D-13).

## 7. Intelligence

A single `ModelProvider` trait with one Anthropic implementation written directly on `reqwest` + server-sent events, a context builder with a hard token budget, and a local intent router that runs before any cloud call.

### 7.1 Provider interface (smallest useful)

```rust
pub struct TurnRequest {
    pub model: String,                 // from config, never hard-coded
    pub system: String,
    pub messages: Vec<Message>,        // provider-neutral types
    pub tools: Vec<ToolSpec>,          // from the registry, filtered by policy
    pub max_output_tokens: u32,
}

pub enum TurnEvent {
    TextDelta(String),
    ToolCall { id: String, name: String, args: serde_json::Value },
    Done { stop: StopReason, usage: Usage },
}

#[async_trait::async_trait]
pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn stream_turn(
        &self,
        req: TurnRequest,
        cancel: CancellationToken,
    ) -> Result<BoxStream<'static, Result<TurnEvent, ProviderError>>, ProviderError>;
}
```

No official Anthropic Rust SDK exists; the available crates are community-maintained \[Verified\]. Owning a small client avoids lag behind API changes.

### 7.2 Turn loop

1. Local intent router: confident match → execute locally, no cloud.
2. Build context: system prompt (persona, capabilities, today's date, offline status) + up to 5 retrieved memories in a quoted "user-approved notes" block + recent turns trimmed to a token budget (default 8k input tokens).
3. Stream; text deltas go to the UI channel and the TTS sentence splitter.
4. Tool call → gateway (§9). At most 5 tool calls per user turn; then the model must answer.
5. Record usage against the spend meter.

### 7.3 Failure behaviour

| Condition | Behaviour |
| --- | --- |
| Connect timeout 5 s / no first token in 10 s / turn over 60 s | Cancel, `PROVIDER_UNAVAILABLE`, offer retry |
| HTTP 429 or 5xx before first token | Retry twice with jittered backoff |
| Any error after a tool has executed | Never retry automatically; report what ran |
| Invalid or missing API key | `PROVIDER_AUTH`, open settings |
| Offline | Local intents only; say so once per session |
| Spend cap reached | `SPEND_CAP_REACHED`; local intents still work |

### 7.4 Cost and disclosure

- Daily and monthly spend caps in settings, enforced in the backend from reported token usage; a running total is visible in settings.
- `cloudInUse` in the snapshot drives a visible indicator whenever a request is in flight.
- Model choice is configuration: default to the fastest current Claude tier for voice latency, with a heavier model selectable per user.
- The persona prompt lives in a versioned file under `src-tauri/resources/prompts/`, reviewed like code.

## 8. Memory

SQLite through `rusqlite` in the backend only, full-text search (FTS5) for retrieval, and nothing becomes a durable memory without the user confirming the exact text.

### 8.1 Storage decisions

- `rusqlite` with the bundled SQLite build; confirm FTS5 is enabled in that build during M5 setup.
- Never use a SQL plugin that exposes queries to the webview; the frontend sees only typed memory commands.
- WAL mode; migrations embedded in the binary and applied at start-up; a failed migration aborts start-up with a clear error and leaves the file untouched.
- No embeddings in alpha (they need another model download). Revisit once FTS retrieval quality has been measured.
- Encryption at rest: rely on OS disk encryption in alpha and say so in the privacy doc (\[Open\] D-10).

### 8.2 Schema (alpha)

| Table | Key columns | Notes |
| --- | --- | --- |
| `conversations` | `id`, `started_at`, `ended_at`, `title` | A conversation ends after 10 min idle or on explicit "new conversation" |
| `messages` | `id`, `conversation_id`, `role`, `content`, `ts`, `tool_call_json` | Tool results stored as summaries, not file contents |
| `memories` | `id`, `content`, `status` (`proposed` / `active` / `archived`), `source_message_id`, `created_at`, `updated_at` | Only `active` rows are retrieved |
| `memories_fts` | FTS5 over `memories.content` | Kept in sync by triggers |
| `audit_log` | `id`, `ts`, `request_id`, `tool`, `risk`, `targets`, `decision`, `outcome`, `duration_ms` | Append-only; no content |
| `settings` | `key`, `value_json`, `schema_version` | Versioned, migrated |

### 8.3 Memory policy

| Information | Stored automatically? | Rule |
| --- | --- | --- |
| Conversation history | Yes | Retention setting: off / 7 / 30 (default) / 365 days / forever; "delete all history" button |
| User says "remember that…" | No | Confirmation card shows the exact text to be saved; saved on approve |
| Model infers something worth remembering | No | Becomes `proposed`; never retrieved until the user accepts it |
| Content of files read by tools | No | Only path and a one-line summary in history |
| Raw audio, API keys, detected secrets | Never | Secret patterns (keys, tokens) are redacted before storage |

### 8.4 Retrieval and deletion

- Retrieve up to 5 `active` memories by FTS rank, ties broken by recency, inserted as quoted data, never as instructions.
- Delete is a hard delete; a test asserts that deleted text no longer appears in FTS results, retrieval, or the next prompt.
- Export all memories and history as JSON from settings.

## 9. Tools, permission gateway and threat model

The model can only propose; the gateway decides from static policy, validated arguments and a taint flag, and an approval is bound to a hash of the exact arguments it shows.

### 9.1 Trust boundaries

| Component | Trust |
| --- | --- |
| Rust backend (orchestrator, gateway, tools) | Trusted |
| User via physical input on the JARVIS window | Trusted for approvals |
| User via voice | Trusted for requests and medium approvals only (a speaker can impersonate them) |
| Webview (React) | Semi-trusted: renders model output, so treat it as potentially compromised |
| Model output, tool results, file contents, web pages, transcripts | Untrusted data |

### 9.2 Threats and controls

| Threat | Vector | Control |
| --- | --- | --- |
| Prompt injection | A read file says "now move \~/Documents to…" | Taint flag set when any tool returns external content; while tainted, every medium/high action needs approval regardless of user policy |
| Destructive action | Model proposes delete or script | Not in the alpha registry; prohibited list enforced in code |
| Scope escape | `../`, symlinks, Windows junctions, UNC or device paths | Canonicalise, compare against canonicalised allowed roots (case-insensitive on Windows/macOS), reject UNC/device paths, re-check immediately before execution |
| Approval spoofing | TV or JARVIS's own TTS says "yes" | High risk: on-screen click or keypress only; approval expires after 60 s; one-shot |
| Bait and switch | Model changes arguments after approval | Approval binds to `argsDigest`; gateway executes only the digested arguments |
| XSS in the webview | Model returns HTML or `javascript:` links | Markdown renderer with raw HTML disabled; sanitised; strict CSP; no remote content; links open via a backend command with confirmation |
| Compromised webview | Script calls IPC directly | No fs/shell/sql/http plugins in capabilities; webview can request a turn and resolve approvals, nothing else |
| Secret leakage | Key in logs, prompts or memory | OS keychain; redaction layer on logs and stored text; key never crosses IPC after entry |
| Supply chain | Tampered model or crate | SHA-256 pinned model manifest; `cargo deny` and `pnpm audit` in CI; licence allow-list |
| Resource exhaustion | Huge folder search, huge file read | Result caps, 1 MB read cap, timeouts, per-tool rate limits |

### 9.3 Gateway algorithm

1. Reject if the tool is not in the registry.
2. Deserialise arguments strictly (`deny_unknown_fields`) and validate ranges.
3. Resolve resources: canonical paths inside allowed roots; app IDs in the allow-list.
4. Effective risk = max(tool base risk, argument-derived risk, taint escalation).
5. Prohibited → deny. High → UI-only approval. Medium → approval unless the user's policy auto-allows it **and** the context is not tainted. Low → run within scope.
6. Approval request carries summary, targets, `argsDigest`, expiry; decision must echo the digest.
7. Execute with timeout and cancellation token; write the audit row (no file content).
8. Return the result to the model wrapped as untrusted data; set the taint flag if it contains external content.

### 9.4 Alpha tool set

| Tool | Base risk | Scope / limits |
| --- | --- | --- |
| `system.status` | Low | CPU, memory, battery, network; sampled every 5 s only while visible |
| `time.now` | Low | — |
| `apps.list_allowed` / `apps.launch` | Low | Only apps the user added to the allow-list in settings; launched with fixed arguments, no shell |
| `files.search` | Low | Filename match inside allowed roots; max 50 results; no content |
| `files.read_text` | Medium | Allowed roots; text types; ≤ 1 MB; approval says the content will be sent to the cloud provider |
| `files.move` / `files.rename` | Medium | Source and destination both inside allowed roots; no overwrite |
| `memory.propose` | Low | Creates a `proposed` memory only |

Not in alpha: delete, shell, scripts, installers, messaging, purchases, settings changes, browser automation.

### 9.5 Tool contract (Rust)

```rust
pub trait Tool: Send + Sync {
    const NAME: &'static str;
    type Args: DeserializeOwned + JsonSchema;   // strict schema exported to the model
    type Output: Serialize;
    fn base_risk(&self) -> Risk;
    fn resolve(&self, args: &Self::Args, policy: &Policy) -> Result<Resolved, ToolError>;
    fn summarise(&self, resolved: &Resolved) -> String;   // human text for the approval card
    async fn run(&self, resolved: Resolved, cancel: CancellationToken) -> Result<Self::Output, ToolError>;
    fn timeout(&self) -> Duration;
}
```

This trait is a sketch: because it has associated types it is not object-safe, so the registry stores type-erased wrappers (`Box<dyn ErasedTool>`) that deserialise JSON arguments into each tool's `Args`.

## 10. Performance budgets

These are engineering budgets, not measurements: hard limits block the alpha, targets are what we tune toward, and all are measured on the reference machines below.

**Reference machines**: (a) x86-64 laptop, 4 cores / 8 threads, 2020–2022 class, 8 GB RAM, integrated graphics, SSD; (b) Apple M1 with 8 GB. If neither developer owns (a), use the slower developer machine and record its spec in `docs/perf.md`.

| Metric | Hard limit (alpha) | Target | Measured how |
| --- | --- | --- | --- |
| CPU, standby, window hidden, wake word on | ≤ 5% of total, 10-min average | ≤ 2% | OS process monitor, scripted |
| CPU, window visible, hologram idle | ≤ 8% | ≤ 4% | Same |
| Resident memory, standby | ≤ 400 MB | ≤ 250 MB | STT/TTS unloaded after 5 min idle |
| Peak memory during a turn | ≤ 1.2 GB | ≤ 800 MB | Same |
| Wake detected → visual acknowledgement | ≤ 400 ms | ≤ 200 ms | Timestamp in logs |
| End of speech → transcript (5 s utterance) | ≤ 1.2 s | ≤ 0.5 s | Fixture audio |
| End of speech → first audio, local intent | ≤ 1.5 s | ≤ 0.8 s | Fixture audio |
| End of speech → first audio, cloud, short question (p50) | ≤ 3.5 s | ≤ 2.0 s | 20 runs, network noted |
| Interrupt → silence | ≤ 300 ms | ≤ 150 ms | Log timestamps |
| Hologram frame rate while animating | ≥ 30 fps | 60 fps | `requestAnimationFrame` sampler |
| Hologram when idle or window hidden | 0 frames rendered | — | Render-on-demand |
| Cold start → interactive | ≤ 4 s | ≤ 2 s | Scripted |
| Installer size (no models) | ≤ 50 MB | ≤ 25 MB | Build artefact |
| Default model set on disk | ≤ 600 MB | ≤ 400 MB | Manifest total |
| Wake word false accepts | ≤ 1 per 4 h of negative audio | ≤ 1 per 24 h | Spike corpus |
| Wake word false rejects (1–3 m, quiet room) | ≤ 10% | ≤ 5% | Spike corpus |
| STT word error rate on team corpus | ≤ 12% | ≤ 7% | Spike corpus |

## 11. Architecture decision records

Seventeen decisions; each becomes `docs/adr/NNN-title.md` with context, decision, alternatives, consequences and a review trigger. Licences marked "check" must be confirmed in the dependency audit before distribution.

| ADR | Decision | Reason | Alternatives considered | Licence | Status |
| --- | --- | --- | --- | --- | --- |
| 001 | Tauri 2 desktop shell | Small binaries, Rust backend, capability-based permissions | Electron: consistent Chromium WebGL but much heavier | MIT/Apache-2.0 | Accepted |
| 002 | React + TypeScript + Vite | Team familiarity, Tauri template | Svelte, Solid | MIT | Accepted |
| 003 | Single crate with modules; workspace only if a sidecar is needed | Simplest build for two people | Multi-crate workspace now | — | Accepted |
| 004 | `sherpa-onnx` official crate as the one speech engine | Covers KWS, VAD, STT, TTS; prebuilt static libs for all targets \[Verified\] | whisper-rs + Piper + separate KWS runner | Apache-2.0 | Accepted, confirm in spike |
| 005 | Wake-word model | Decided by measured FA/FR and licence | See §6.3 | Varies | Open (D-02) |
| 006 | STT model: Moonshine base or Whisper base.en/small.en | Smallest model meeting WER and latency | Parakeet, Zipformer streaming | MIT (English Moonshine, Whisper) | Open (D-04) |
| 007 | TTS: Kokoro-82M `bm_george`/`bm_lewis` with GPL-free G2P | Apache-2.0 voice, British male packs | Piper (GPL-3.0 engine), Kitten, Matcha | Apache-2.0; G2P check | Proposed (D-03) |
| 008 | `cpal` for capture and playback | De-facto Rust cross-platform audio | PortAudio bindings, `rodio` for playback only | Check | Accepted |
| 009 | AEC deferred; alpha uses interrupt paths | Avoid an immature dependency in the critical path | `sonora` AEC3 port, OS voice processing | BSD-3-Clause (`sonora`) | Open (D-05) |
| 010 | IPC: commands + Channels + broadcast events; generated TS types | Typed, fast streaming, one source of truth | Events for everything; hand-written types | — | Accepted |
| 011 | Own Anthropic client on `reqwest` + SSE behind `ModelProvider` | No official Rust SDK \[Verified\]; small surface | Community SDK crates | MIT/Apache-2.0 | Accepted |
| 012 | SQLite via `rusqlite`, FTS5, no webview SQL | Local-first, searchable, no extra service | `sqlx`, SQL plugin, embedded vector DB | MIT | Accepted |
| 013 | `keyring` crate for the API key; session-only fallback where no secret service exists | Native OS stores on all three platforms | Encrypted file with a user password | Check | Accepted |
| 014 | Zustand store as projection of backend snapshots; Zod only for user-supplied data | One state machine; less duplication | Redux, XState in the frontend | MIT | Accepted |
| 015 | Three.js + R3F + Drei with on-demand rendering; mandatory 2D (SVG/CSS) fallback; Linux defaults to 2D until measured | Average laptops; WebKitGTK WebGL can silently be slow and masks the GPU name \[Verified\] | Canvas 2D only; Lottie | MIT | Accepted |
| 016 | Browser automation deferred beyond alpha | Node + browser binaries; large attack surface | Playwright sidecar | Apache-2.0 | Accepted (deferred) |
| 017 | `tracing` with rotating local JSON logs, redaction layer, no remote telemetry | Debuggable without leaking private content | Remote crash reporting | MIT | Accepted |

## 12. Refined repository structure

The generated Tauri template stays intact; folders marked with a milestone are created only when that milestone starts.

```text
jarvis/
├── CLAUDE.md
├── README.md
├── docs/
│   ├── spec.md                  # this document, exported
│   ├── decisions.md             # running decision log (one line per decision)
│   ├── adr/                     # 001-tauri.md … 017-logging.md
│   ├── contracts.md             # human-readable IPC contract + examples
│   ├── threat-model.md
│   ├── perf.md                  # reference machines + measured results
│   └── setup-{windows,macos,linux}.md
├── src/                         # frontend (Developer B)
│   ├── main.tsx
│   ├── App.tsx
│   ├── transport/
│   │   ├── index.ts             # Transport interface
│   │   ├── tauri.ts             # TauriTransport (commands, channels, events)
│   │   └── mock/                # MockTransport + scripts/*.json (dev only)
│   ├── types/generated/         # generated from Rust; never edit by hand
│   ├── stores/                  # snapshot store, view-only UI store
│   ├── features/
│   │   ├── assistant/           # conversation, input, status
│   │   ├── hologram/            # 3D (R3F) and 2D fallback, same props
│   │   ├── activity/            # audit list, approval cards (wired in M4)
│   │   ├── memory/              # M5
│   │   └── settings/
│   ├── components/common/
│   ├── styles/                  # tokens.css, typography
│   └── test/
├── src-tauri/                   # backend (Developer A unless noted)
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── Info.plist               # NSMicrophoneUsageDescription
│   ├── Entitlements.plist       # audio-input (signed builds)
│   ├── capabilities/default.json  # minimal: core + our commands only
│   ├── resources/prompts/       # persona prompt, versioned
│   └── src/
│       ├── main.rs
│       ├── lib.rs               # builder, command registration
│       ├── config/              # settings schema + migration
│       ├── ipc/                 # commands, channels, events, protocol version, type export
│       ├── orchestrator/        # state machine, turns, cancellation
│       ├── audio/               # M3 (spike code lives in spikes/ until then)
│       ├── voice/               # M3: wake, vad, stt, tts traits + sherpa impls
│       ├── intelligence/        # M2: provider, anthropic, context, intents, spend
│       ├── tools/               # M4: registry + tools
│       ├── permissions/         # M4: policy, gateway, approvals, audit
│       ├── memory/              # M5: db, migrations, repos
│       ├── platform/            # adapters per OS (Developer B with A review)
│       └── models/              # manifest, downloader, checksum
├── models/manifest.json         # URLs, sizes, SHA-256, licences
├── spikes/                      # throwaway CLI experiments (wake word, TTS, AEC)
├── tests/fixtures/audio/        # consented recordings, Git LFS, never shipped
└── .github/workflows/ci.yml
```

## 13. Milestone roadmap and acceptance tests

Seven milestones and one parallel spike, reordered so the riskiest unknowns (wake word, voice) are tested in weeks, not months, and each milestone ends with something runnable; the roadmap drawing follows the lists.

| Milestone | Goal | Depends on | Lead |
| --- | --- | --- | --- |
| M0 Foundation | Reproducible app on 3 OSes, contract skeleton, CI | — | Both |
| S1 Voice spikes (parallel with M1) | Decide wake word, STT, TTS, AEC approach with data | M0 | A |
| M1 Interface | Full shell on `MockTransport` | M0 | B |
| M2 Brain (text) | Useful text assistant with streaming and local intents | M0, contracts | A + B |
| M3 Voice | Local voice pipeline driving M2 | S1 gate, M2 | A (B: audio UI) |
| M4 Hands | Gateway, approvals, alpha tools | M2 | A (B: approvals UI, platform adapters) |
| M5 Memory | History, consented memories, retrieval | M2, M4 approval card | A + B |
| M6 Alpha | Hardened, profiled, packaged | M3–M5 | Both |

### M0 Foundation — acceptance

- [ ] `pnpm tauri dev` starts on both developers' machines and in CI on Windows, macOS and Ubuntu runners.
- [ ] `ping` returns `{ pong, protocolVersion, appVersion }`; a frontend test calls it through `TauriTransport`.
- [ ] `get_snapshot` returns the initial `Snapshot`; generated TS types compile; CI fails if generated types are stale.
- [ ] Structured logs written to the app log dir with rotation; a test asserts the redaction layer masks a fake API key.
- [ ] `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `pnpm typecheck`, `pnpm test`, `pnpm format:check` all pass in CI.
- [ ] `capabilities/default.json` grants nothing beyond core and our commands (reviewed by both).
- [ ] `docs/decisions.md` records D-01 (distribution intent).

### S1 Voice spikes — exit gate G1

- [ ] Team corpus recorded with consent: ≥ 200 "hey jarvis" utterances per developer at 1 m and 3 m, ≥ 4 h negative audio (TV, podcasts, conversation).
- [ ] Each wake-word candidate measured: FA per hour and FR rate at three thresholds; CPU on reference machine.
- [ ] STT candidates: WER and latency on 100 command utterances.
- [ ] TTS: blind audition by ≥ 5 listeners; real-time factor on reference machine; G2P checked on a 200-word list including names.
- [ ] Gate G1 passes if at least one licence-compatible option per stage meets the §10 hard limits; otherwise re-plan before M3.

### M1 Interface — acceptance

- [ ] All `MicState` × `AssistantPhase` combinations render distinctly with text labels, not colour alone.
- [ ] Scripted mock runs: wake → listen → think → speak; interrupt; error; offline; approval request.
- [ ] 2D fallback renders when WebGL is unavailable, disabled in settings, or on Linux by default.
- [ ] `prefers-reduced-motion` stops non-essential animation; hologram renders 0 frames when idle or hidden.
- [ ] Keyboard-only: every control reachable with visible focus; Esc cancels; mute has a shortcut.
- [ ] Screen reader announces assistant replies (`aria-live="polite"`) and state changes.
- [ ] Usable at 1280×720 and 1920×1080 and in compact orb mode; persistent MOCK badge visible.

### M2 Brain — acceptance

- [ ] API key stored in the OS keychain; never visible in frontend state, logs or IPC after entry.
- [ ] Streaming reply renders token by token; cancel stops the HTTP stream within 300 ms.
- [ ] Local intents in §3.3 answer with the network disabled.
- [ ] Fault tests: no key, wrong key, offline, 429, 500, timeout — each produces the correct error code and recovers.
- [ ] Spend cap blocks cloud calls at the limit; usage visible in settings.
- [ ] Model-output markdown with `<script>`, `<img onerror>` and `javascript:` links renders inert (test).

### M3 Voice — acceptance

- [ ] Wake word and push-to-talk start a turn; transcript appears; reply is spoken in the chosen voice.
- [ ] Budgets in §10 for wake, transcript, first audio and interrupt met on reference machines.
- [ ] Unplug/replug the mic during standby and during capture: no crash, recovery without restart.
- [ ] Mic permission denied on macOS and Windows shows `MIC_PERMISSION_DENIED` with guidance.
- [ ] Network capture during standby shows zero outbound traffic from the app (pre-wake audio stays local).
- [ ] No audio file is created anywhere during an 8-hour soak; no native crash.

### M4 Hands — acceptance

- [ ] Each alpha tool has unit tests for valid input, invalid input, out-of-scope paths, timeout and cancellation.
- [ ] Path-escape suite passes: `..`, symlink out of root, Windows junction, UNC, case variants, file swapped between approval and execution.
- [ ] Injection test: a file containing instructions to move files triggers taint; the move is proposed with an approval card, never auto-run.
- [ ] Changing arguments after approval is rejected (digest mismatch test).
- [ ] High-risk approvals cannot be granted by voice (test with synthetic "yes" audio).
- [ ] Audit log lists every tool call with decision and outcome, without file contents.

### M5 Memory — acceptance

- [ ] "Remember that…" shows a confirmation card; nothing is stored on deny.
- [ ] Proposed memories are never retrieved until accepted.
- [ ] Edit and delete work; deleted text is absent from FTS, retrieval and the next prompt (test).
- [ ] Retention setting purges history on schedule; "delete all" leaves an empty database.
- [ ] Migration from an M2-era database succeeds; a corrupt database is backed up and replaced with a clear message.

### M6 Alpha — acceptance

- [ ] Installers build for all three OSes from CI; first-run model download with checksum verification and offline import; the macOS ad-hoc build installs on a second Mac using the documented Open Anyway steps.
- [ ] Every §10 hard limit met and recorded in `docs/perf.md`.
- [ ] Security checklist (§14) complete; dependency and model licence audit complete.
- [ ] Start at login, tray, quit, and crash-restart behave correctly on all three OSes.
- [ ] Setup, privacy and known-limitations docs written for a non-developer reader.

&#91;embedded content: roadmap · 2 lanes, 7 phases, 2 gates\]

G1 passes only if a licence-clean option for each voice stage meets the §10 hard limits; G2 passes when the §14 checklist and the licence audit are complete.

## 14. Security and privacy checklist

Every item is checked before alpha; items marked (M0) are enforced from the first commit.

### Privacy

- [ ] (M0) No telemetry, analytics or crash reports leave the machine.
- [ ] Pre-wake audio processed only in RAM on the voice thread; verified by a network capture during standby.
- [ ] No audio written to disk by the app in any mode; test corpus recorded only with the separate `spikes/` tool.
- [ ] Visible mic state at all times, including in compact orb and tray icon; one-click mute.
- [ ] Visible indicator whenever data is sent to the cloud provider; settings page names the provider and what is sent.
- [ ] File reads that send content to the cloud say so on the approval card.
- [ ] History retention setting, delete-all, and JSON export work.
- [ ] Durable memories require explicit confirmation of the exact text.
- [ ] Privacy doc lists every on-disk location (database, logs, models, settings).

### Secrets

- [ ] (M0) No API keys in source, `.env` files committed to Git, or any `VITE_*` variable.
- [ ] API key in the OS keychain; session-only fallback where no secret service exists, with a warning.
- [ ] Redaction layer covers logs, stored messages and memory text.

### Webview and IPC

- [ ] (M0) Capabilities: core + our commands only; no fs, shell, http, sql or opener plugins exposed to the webview.
- [ ] Strict CSP; no remote scripts, fonts or images at runtime.
- [ ] Model output rendered without raw HTML; XSS test suite passes.
- [ ] Links open only through a backend command with confirmation and an `http(s)` scheme check.
- [ ] Every command validates its input in Rust.

### Tools and permissions

- [ ] No unrestricted shell or filesystem tool exists in the registry.
- [ ] Gateway enforced in Rust; no code path from model output to a tool bypasses it (reviewed by both developers).
- [ ] Allowed roots canonicalised; path-escape suite passes on all three OSes.
- [ ] Approval binds to `argsDigest`, expires after 60 s, is single-use.
- [ ] High-risk approvals UI-only; taint escalation tested.
- [ ] Every tool has a timeout and honours cancellation.
- [ ] Audit log complete and contains no file content.

### Supply chain and distribution

- [ ] `cargo deny` (licences + advisories) and `pnpm audit` in CI.
- [ ] Licence allow-list matches the distribution decision (D-01); no NC or GPL component in a non-GPL release.
- [ ] Model manifest pins SHA-256 for every file; downloads verified before load.
- [ ] macOS: ad-hoc signed with hardened runtime and the audio-input entitlement; install steps documented; no notarisation (D-13). Windows: unsigned for alpha.
- [ ] Name and visual identity checked before any public release (D-14).

## 15. Unresolved decisions and experiments

Sixteen decisions remain open; D-01 must be made first because it filters the licences every other voice decision can use.

| ID | Decision | Experiment or input | Decide when | Needed by | Owner |
| --- | --- | --- | --- | --- | --- |
| D-01 | Distribution intent: personal / open source (which licence) / commercial | Team discussion; write it in `decisions.md` | Licence allow-list follows directly | End of M0 | Both |
| D-02 | Wake-word engine and model | S1 corpus: FA/h and FR% at 3 thresholds, CPU, per candidate | Lowest FR that meets FA ≤ 1/4 h and passes D-01 | Gate G1 | A |
| D-03 | TTS voice and phonemizer | Blind audition (≥ 5 listeners), RTF on reference laptop, 200-word G2P list | Best-rated voice meeting first-audio budget with a licence-clean G2P | Gate G1 | A |
| D-04 | STT model | WER + latency on 100 team commands | Smallest model under WER and latency limits | Gate G1 | A |
| D-05 | Echo cancellation | Play TTS through laptop speakers, measure wake-word FA and transcript corruption with/without `sonora` and OS voice processing | Adopt only if it removes self-triggering without breaking §10 CPU limits | Before M6 | A |
| D-06 | In-process vs sidecar voice | 8-hour soak + fault injection (corrupt model, device yank) | Any native crash → sidecar | End of M3 | A |
| D-07 | TS type generator | Generate the M0 contract with `ts-rs` and with `tauri-specta` (exact pins) | Least friction with stable releases | M0 | Both |
| D-08 | Linux hologram default | Measure fps on Intel/AMD/NVIDIA Linux machines available to the team | 3D default only where ≥ 30 fps measured | M1 | B |
| D-09 | Keychain fallback on Linux without Secret Service | Test on a minimal desktop | Session-only vs encrypted file with passphrase | M2 | A |
| D-10 | Database encryption at rest | Cost of SQLCipher builds on 3 OSes | Adopt only if D-01 targets users without disk encryption | M5 | A |
| D-11 | History retention default | Privacy review | 30 days unless the team prefers off | M5 | Both |
| D-12 | Update delivery | Tauri updater with signed manifests; model manifest hosting | Simplest signed path for both app and models | M6 | Both |
| D-13 | Code signing (resolved 31 Oct 2026) | No Apple Developer membership: macOS ad-hoc signed, not notarised; Windows unsigned for alpha | Revisit only before a public or commercial release (D-01) | M6 | Both |
| D-14 | Public name and visual identity | Trademark and IP check if D-01 ≠ personal | Rename before first public release if risk found | Before public release | Both |
| D-15 | Screen-reader support level | Test with NVDA, VoiceOver, Orca | Replies, states and approvals fully announced | M1 / M6 | B |
| D-16 | Local LLM provider post-alpha | Measure a small quantised model's latency on reference laptop | Only if it answers in ≤ 5 s on CPU | After alpha | A |

## 16. CLAUDE.md

Place this at the repository root; it is also attached in the conversation as a ready file. It assumes the `pnpm` scripts `typecheck`, `lint`, `format:check` and `gen:types` that M0 tasks A-04 and B-02 create.

````markdown
# CLAUDE.md — JARVIS

Cross-platform, voice-first desktop assistant. Tauri 2 + Rust backend, React + TypeScript frontend.
The full specification is `docs/spec.md`; decisions are in `docs/decisions.md` and `docs/adr/`.

## Before you change anything
1. Read `docs/spec.md` sections relevant to the task, `docs/decisions.md`, and the files you will touch.
2. State a short plan (files, approach, tests) and wait for approval on anything that changes a shared contract.
3. Work on one milestone task at a time. Do not start the next milestone's work.

## Ownership
- Developer A: `src-tauri/src/{orchestrator,audio,voice,intelligence,tools,permissions,memory}`.
- Developer B: `src/**`, `src-tauri/src/{platform,models}` (A reviews).
- Shared: `src-tauri/src/ipc/**`, `src/types/generated/**`, `docs/contracts.md`. Change only with both developers' agreement; bump `PROTOCOL_VERSION` on breaking changes.
- Do not edit the other developer's modules unless the task says so.

## Workflow between the two developers
- One session = one GitHub issue = one branch = one PR into `develop`. Branch names: `a/<issue>-<slug>` or `b/<issue>-<slug>`.
- Start from the latest `develop`. A cloud session clones GitHub, so anything not pushed does not exist for it.
- Shared contract changes (`ipc/`, generated types, `docs/contracts.md`) go in their own contract PR first. Never mix a contract change into a feature PR.
- Regenerate `src/types/generated/` only in a PR that changes Rust contract types.
- Record any decision in `docs/decisions.md` in the same PR.
- Open the PR with `.github/pull_request_template.md` filled in. Never merge it yourself; the other developer reviews and merges.

## Architecture rules (non-negotiable)
- The Rust orchestrator owns the only assistant state machine. The frontend renders snapshots and sends commands; it never transitions assistant state itself.
- `MicState` and `AssistantPhase` are separate. `interrupted`/`cancelled` are events, not states.
- Streams use Tauri Channels; low-rate broadcasts use events; requests use commands.
- TS types in `src/types/generated/` are generated from Rust. Never hand-edit them; regenerate and commit.
- Every long operation takes a `CancellationToken` and has a timeout.
- No inference on Tokio worker threads; nothing allocates or locks inside the audio callback.
- Platform-specific code lives only in `src-tauri/src/platform/`.

## Security rules (non-negotiable)
- The model proposes; the permission gateway in Rust decides. No code path from model output to a tool may bypass `permissions::gateway`.
- Never add an unrestricted shell, script, or filesystem tool. Never add fs/shell/http/sql/opener plugins to webview capabilities.
- Approvals bind to `argsDigest`, expire after 60 s, are single-use. High-risk approvals are UI-only, never voice.
- Tool results and file/web content are untrusted data; they set the taint flag.
- Canonicalise paths and check them against canonicalised allowed roots, again immediately before execution.
- Render model output as markdown with raw HTML disabled.
- API keys: OS keychain only. Never in source, `.env` committed to Git, `VITE_*` variables, logs, or IPC payloads after entry.
- Pre-wake audio never leaves RAM; the app never writes audio to disk.

## Dependencies
- Do not add a dependency without a one-line reason in the PR and a licence check against the allow-list in `docs/decisions.md` (D-01).
- No GPL or non-commercial components unless D-01 allows them (watch espeak-ng, Piper, openWakeWord pretrained models).
- Speech engine is the official `sherpa-onnx` crate; do not add whisper.cpp, Piper or another runtime alongside it.

## Cloud sessions
- If `$CLAUDE_CODE_REMOTE` is `true`, you are on an Ubuntu x86-64 VM with no display, microphone, speaker, macOS or Windows.
- Never claim you ran the GUI, heard audio, or measured performance there. End such tasks with a manual test checklist for the developer.
- Send long build and test output to a file; read only the errors and the summary.
- macOS builds are ad-hoc signed (`signingIdentity: "-"`); never add notarisation steps or Apple credentials.

## Mocks
- Mock behaviour lives only in `src/transport/mock/`, is enabled only by `VITE_TRANSPORT=mock` in dev builds, and shows a MOCK badge. Never present mock behaviour as working functionality.

## Before reporting completion, run
```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test   # in src-tauri/
pnpm typecheck && pnpm lint && pnpm test && pnpm format:check
pnpm gen:types && git diff --exit-code src/types/generated
```

## Completion report (always)
- Files changed (one line each, why).
- Commands run and their results.
- What is implemented, what is mocked, what is not done, and what needs a manual test on real hardware.
- Limitations, risks, and any decision that needs a human (add to `docs/decisions.md` as "proposed").

## Cost discipline
- Read only the files the task needs. Prefer small, reviewable diffs. Do not refactor outside the task.
````

## 17. First implementation tasks

Each task is one Claude Code session and one pull request into `develop`; IDs are referenced in commits. Start with the three shared tasks, then work in parallel.

### Shared (both developers, before parallel work)

- [ ] **S-01** Decide D-01 (distribution intent) and record the licence allow-list in `docs/decisions.md`.
- [ ] **S-02** Review and agree `docs/contracts.md` v1 (Snapshot, envelope, errors, command list) before A-03 and B-03 merge.
- [ ] **S-03** Agree branch rules: feature branches → `develop` via PR with the other developer's review; `main` only from tested `develop`.

* [ ] **S-04** Create the cloud environment in §18.1 (setup script + SessionStart hook) and confirm a cloud session can run `cargo check` and `pnpm test`.

### Developer A — backend (M0, then spike S1)

- [ ] **A-01** Bootstrap with `pnpm create tauri-app` exactly as brief §10, push to `main`, create `develop`.
- [ ] **A-02** Add Rust modules `config`, `ipc`, `orchestrator`; configure `tracing` with rotating JSON file logs and a redaction layer; test that a fake key is masked.
- [ ] **A-03** Rust types `Snapshot`, `MicState`, `AssistantPhase`, `JarvisError`, `Envelope`; commands `ping` and `get_snapshot`; unit tests.
- [ ] **A-04** Type generation trial for D-07 (`ts-rs` vs pinned `tauri-specta`); add `pnpm gen:types`; CI drift check.
- [ ] **A-05** Orchestrator transition table as pure logic (no audio, no network) with tests for every legal and a sample of illegal transitions; emit `state.snapshot` on change.
- [ ] **A-06** Minimal `capabilities/default.json`, strict CSP in `tauri.conf.json`, `Info.plist` with `NSMicrophoneUsageDescription`.
- [ ] **A-07** CI on Windows, macOS and Ubuntu: fmt, clippy `-D warnings`, tests, `cargo deny`.
- [ ] **A-08** `spikes/voice-cli`: `cpal` capture → `sherpa-onnx` KWS, VAD, STT, TTS from the terminal; print latency and CPU per stage. No app integration.
- [ ] **A-09** `spikes/record-corpus`: consented recording tool with metadata (speaker, distance, room, mic); record the S1 corpus.
- [ ] **A-10** Run D-02, D-03, D-04 evaluations; write `docs/spikes/s1-report.md`; hold gate G1.

A-08 to A-10 are written in a cloud session but run by a developer on their own laptop, because cloud VMs have no microphone, speaker or display. Each of those PRs ends with the exact commands to run and what numbers to paste back.

### Developer B — frontend (M0, then M1)

- [ ] **B-01** Inspect the generated `package.json`; add only the brief's UI and test dependencies plus ESLint, each with a one-line reason in the PR.
- [ ] **B-02** Scripts `typecheck`, `lint`, `test`, `format`, `format:check`; Vitest with jsdom; add to CI.
- [ ] **B-03** `Transport` interface and `TauriTransport` for `ping`, `get_snapshot` and snapshot subscription; tests with a mocked `invoke`.
- [ ] **B-04** `MockTransport` replaying `scripts/*.json` envelopes; persistent MOCK badge; excluded from release builds.
- [ ] **B-05** Zustand snapshot store that accepts only higher `seq`; tests for stale and out-of-order snapshots.
- [ ] **B-06** Design tokens (colour, type, spacing, motion) in `styles/tokens.css`; WCAG AA contrast check for text tokens.
- [ ] **B-07** App shell: hologram area, conversation panel, status bar, activity placeholder, settings shell, compact orb mode.
- [ ] **B-08** Hologram: 2D SVG version first, then R3F version with on-demand rendering behind the same props; reduced-motion; Linux defaults to 2D.
- [ ] **B-09** Conversation panel with streaming text and safe markdown (raw HTML off); XSS test cases from §13 M2.
- [ ] **B-10** Status indicators for every `MicState` × `AssistantPhase` with text labels and `aria-live` announcements.
- [ ] **B-11** Approval card component on mock data (summary, targets, risk, expiry countdown, approve/deny, UI-only badge for high risk).
- [ ] **B-12** First Rust task: `platform::system_status` using a system-info crate, exposed as a command; A reviews. Prepares `system.status` for M2 local intents.

### Copy-paste prompt for any task

```text
Task <ID> from docs/spec.md §17. Read CLAUDE.md, the spec sections it references, and only the files you need.
Give a short plan first (files, approach, tests). Then implement, run the checks listed in CLAUDE.md,
and report changed files, results, what is mocked or not done, and remaining risks.
Do not touch files outside your ownership area or start other tasks.
```

## 18. Budget and non-Claude costs

The $200 Claude Code allowance is reallocated toward the spikes and the security work, and three costs outside it are made explicit.

| Area | Brief allocation ($) | Revised ($) | Change reason |
| --- | --- | --- | --- |
| Foundation and contracts (M0) | 20 | 20 | — |
| Voice spikes (S1) | — | 25 | New: cheap CLI experiments before integration |
| Frontend and visualisation (M1) | 35 | 30 | Approval UI designed early, wired later |
| Brain, text (M2) | 40 (with memory) | 20 | Own small client, no SDK |
| Voice integration (M3) | 45 | 25 | Spikes absorb the unknowns |
| Hands and security (M4) | 35 | 35 | Unchanged; security tests are the expensive part |
| Memory (M5) | (in AI) | 15 | FTS only, no embeddings |
| Hardening, packaging, contingency (M6) | 25 | 30 | Cross-platform packaging is where surprises land |
| **Total** | **200** | **200** |  |

Costs outside the allowance:

- **Runtime API usage**: billed separately by the provider; enforced by the in-app spend cap. Record a monthly limit in `decisions.md`.
- **macOS distribution**: no Apple Developer membership, so builds are ad-hoc signed and each tester approves the app once (§6.9); a public macOS release would need a paid membership later.
- **Windows signing**: unsigned installers trigger SmartScreen warnings; alpha installers stay unsigned and testers choose More info, then Run anyway (D-13).

Spending discipline: run spikes as standalone CLIs (small context), give each session one §17 task, and keep `docs/decisions.md` current so sessions do not re-derive decisions.

### 18.1 Using the $200 of cloud sessions

Cloud sessions run on an Ubuntu 24.04 x86-64 VM with about 4 vCPUs, 16 GB RAM and 30 GB disk, with Rust, Node 22 and pnpm pre-installed, and they draw on Claude Code usage like any other session \[Verified\]. They have no display, microphone, speaker, macOS or Windows, so split the work this way:

| Work | Where it runs |
| --- | --- |
| Rust logic: orchestrator, contracts, gateway, tools, memory, unit tests | Cloud session |
| React components and Vitest tests (mock transport) | Cloud session |
| `cargo check` / `cargo build` of the Linux app | Cloud session, with the setup script below |
| CI workflow files | Written in a cloud session; run on GitHub Actions (macOS, Windows, Linux runners) |
| Voice spikes, audio devices, latency and CPU budgets | Code written in the cloud; run by a developer locally |
| Running the GUI, hologram fps, keyboard and screen-reader checks | Developer's machine |
| macOS and Windows builds | GitHub Actions runners or the developers' own machines |

**Network.** The default Trusted access reaches GitHub (including release downloads), crates.io and npm \[Verified\], which covers the build, including `sherpa-onnx`'s prebuilt libraries. Hugging Face is not on the default list: add it under Custom access only for a task that genuinely needs a model file, and keep unit tests model-free.

**Environment setup script** (runs as root; it is cached only if it finishes in about five minutes \[Verified\]). Package list from Tauri's Linux prerequisites plus ALSA for `cpal`; confirm against the current Tauri docs.

```bash
#!/bin/bash
set -e
apt-get update
apt-get install -y libwebkit2gtk-4.1-dev build-essential libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev libasound2-dev pkg-config
```

Put `pnpm install --frozen-lockfile` and `cargo fetch` in a repo SessionStart hook script that exits early unless `CLAUDE_CODE_REMOTE=true`, so it runs in cloud sessions only.

**Spending rules for cloud sessions**

1. One §17 task per session; start a new session for the next task instead of continuing a long one.
2. Send long build and test output to a file and read only the errors and summary; build logs are the biggest avoidable token cost.
3. Never ask a cloud session to "check it works" for GUI, audio or performance; ask for a manual test checklist instead.
4. Plan in the session first and approve the plan before it writes code, so rework is caught early.

## 19. Team workflow

The two developers' Claude Code sessions coordinate only through GitHub: each session starts from a fresh clone, so whatever is not committed and pushed does not exist for the other side.

### 19.1 Shared context lives in the repo

| File | Role |
| --- | --- |
| `CLAUDE.md` | Rules, ownership, workflow; every session reads it automatically |
| `docs/spec.md` | This specification, exported as Markdown |
| `docs/decisions.md` | One line per decision, so no session re-argues it |
| `docs/contracts.md` + `src/types/generated/` | The interface between backend and frontend |
| `.github/pull_request_template.md` | The handoff note every session fills in |
| `.github/CODEOWNERS` | Routes every PR to the other developer for review |

### 19.2 The unit of work

One task = one GitHub issue = one branch = one cloud session = one PR into `develop`.

1. Each §17 task becomes a GitHub issue assigned to one developer.
2. Start the session with "Do issue #N"; cloud sessions can read issues through their built-in GitHub tools.
3. Branch names: `a/<issue>-<slug>` or `b/<issue>-<slug>`, based on the latest `develop`.
4. The session opens a PR using the template; the other developer reviews and merges it.

### 19.3 Contract first, then in parallel

1. Contract PR: Rust types, regenerated TS types and `docs/contracts.md` change together; both developers agree; merge.
2. Developer A implements the backend; Developer B builds the UI against `MockTransport`, both from the merged contract.
3. B switches the feature to `TauriTransport` once the backend PR merges.

Only a PR that changes Rust contract types regenerates `src/types/generated/`; two branches regenerating them at once is the most likely merge conflict.

### 19.4 GitHub settings

- Protect `develop` and `main`: require a pull request, 1 approval, review from code owners, and passing CI; block force-pushes.
- `CODEOWNERS` lists both developers for every path. GitHub never counts a PR's author as its code-owner reviewer, so this always requests the *other* developer. Who writes which module is set in `CLAUDE.md`, not here.
- Merge order for a cross-cutting feature: contract PR, then backend PR, then frontend wiring PR.

### 19.5 Daily rhythm

- Push before starting a cloud session; it clones what is on GitHub, not your laptop.
- Review the other developer's open PRs before starting new work, so `develop` does not drift.
- Hardware checks: the PR lists manual test steps; whoever runs them pastes the results as a PR comment before merge.

## Sources

Checked 31 Oct 2026. Facts not listed here are recommendations or open questions, not verified claims.

- [sherpa-onnx Rust crate docs (v2.0.1)](https://docs.rs/sherpa-onnx) — official crate, licence, platforms, supported model families
- [sherpa-rs listing](https://docsearch.algolia.com/mcp/docs/repo/thewh1teagle/sherpa-rs) — deprecated in favour of the official Rust API
- [sherpa-onnx keyword spotting](https://k2-fsa.github.io/sherpa/onnx/kws/index.html) — open-vocabulary KWS
- [sherpa-onnx KWS pretrained models](https://k2-fsa.github.io/sherpa/onnx/kws/pretrained_models/index.html)
- [openWakeWord pretrained models README](https://huggingface.co/davidscripka/openwakeword/blob/main/README.md) — CC BY-NC-SA 4.0
- [Piper TTS review (2026)](https://www.promptquorum.com/power-local-llm/piper-tts-review) — GPL-3.0-or-later, archive date of rhasspy/piper
- [piper-tts on PyPI metadata](https://mcp.depscope.dev/pkg/pypi/piper-tts) — GPL-3.0-or-later
- [piper-phonemize README](https://docs.rs/crate/piper-phonemize/latest/source/README.md) — GPL-3.0 due to espeak-ng
- [Kokoro-82M model card](https://huggingface.co/hexgrad/Kokoro-82M/blob/refs%2Fpr%2F44/README.md) — Apache-2.0, `bm_george`, `bm_lewis`
- [floravox-cli README](https://docs.rs/crate/floravox-cli/0.7.2) — GPL-free G2P path via `misaki-rs` (MIT)
- [Moonshine repository](https://awesome.ecosyste.ms/projects/github.com%2Fmoonshine-ai%2Fmoonshine) — English models MIT, non-English community licence
- [Tauri: Calling the frontend from Rust](https://v2.tauri.app/develop/calling-frontend) — events vs Channels
- [Tauri: Linux graphics issues](https://v2.tauri.app/develop/debug/linux-graphics/) — WebKitGTK WebGL slow paths, masked renderer
- [tauri-specta docs](https://docs.rs/tauri-specta/2.0.0-rc.21) and [package overview (rc.25)](https://openapps.pro/packages/tauri-specta) — still release candidate
- [Tauri global-hotkey Wayland issue](https://github.com/tauri-apps/tao/issues/331) and [pot Wayland notes](https://pot-app.com/en/docs/wayland.html) — no global shortcuts on Wayland
- [Tauri macOS application bundle](https://v2.tauri.app/de/distribute/macos-application-bundle/) — Info.plist merge, entitlements
- [WarzonePhone Info.plist note](https://git.manko.yoga/manawenuz/wz-phone/src/branch/main/desktop/src-tauri/Info.plist) — missing mic description yields silent zeros
- [VOXCTL Tauri app](https://github.com/dericksozo/voxctl.com) — hardened runtime with `audio-input` entitlement
- [sonora crate](https://docs.rs/crate/sonora/latest) — pure-Rust WebRTC AEC3, BSD-3-Clause, v0.1.0
- [anthropic-rs (unofficial)](https://docs.rs/anthropic-rs) and [anthropic-ai-sdk (unofficial)](https://docs.rs/crate/anthropic-ai-sdk) — community Rust SDKs

* [Tauri: macOS code signing](https://v2.tauri.app/distribute/sign/macos/) — ad-hoc signing with `-`; signature required on Apple Silicon
* [BuffBrain release notes](https://github.com/eulogik/buffbrain/releases) — ad-hoc signing must happen before the DMG is created
* [Macworld: opening unsigned apps in Sequoia](https://www.macworld.com/article/2457844/what-to-do-when-you-cant-open-an-app-you-just-installed-in-macos-sequoia.html) and [AppleInsider](https://appleinsider.com/articles/24/08/06/apple-removes-control-click-option-for-skipping-gatekeeper-in-macos-sequoia) — Control-click bypass removed; Open Anyway in Settings
* [Claude Code cloud sessions](https://code.claude.com/docs/en/claude-code-on-the-web.md) — usage, isolation, limitations
* [Claude Code cloud environments](https://code.claude.com/docs/en/cloud-environments.md) — VM image and resources, installed tools, setup scripts, Trusted domains
