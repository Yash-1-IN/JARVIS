# CLAUDE.md — JARVIS

Cross-platform, voice-first desktop assistant. Tauri 2 + Rust backend, React + TypeScript frontend.
The full specification is `docs/spec.md`; decisions are in `docs/decisions.md` and `docs/adr/`.

## Before you change anything
1. Read `docs/spec.md` sections relevant to the task, `docs/decisions.md`, and the files you will touch.
2. State a short plan (files, approach, tests) and wait for approval on anything that changes a shared contract.
3. Work on one milestone task at a time. Do not start the next milestone's work.

## Ownership
- Developer A: `src-tauri/src/{orchestrator,audio,voice,intelligence,tools,permissions,memory,wake}`.
- Developer B: `src/**`, `src-tauri/src/{platform,models}` (A reviews).
- Developer C (`Jarvis-C-Dev`, an Antigravity agent, in the cloud or on the owner's computer, always under its own `Jarvis-C-Dev` account and token, never the owner's login), three lanes, decision D-32:
  1. the visual layer of the window: `src/styles.css` (or `src/styles/**`), presentation-only changes in `src/components/**` (class names and markup, no behaviour), `src/state/layoutPref.ts`;
  2. release and CI help: `scripts/**`, `docs/setup-*.md`, `docs/first-run.md`, installer and first-run checks (not `.github/workflows/**` security settings, `src-tauri/capabilities/**`, `Entitlements.plist` or `tauri.conf.json`);
  3. test infrastructure: `e2e/**` against the mock transport, regression tests for fixed bugs, measurement scripts.
- Developer D (`backendcodinga`): audits and tests; opens no feature PRs.
- Shared: `src-tauri/src/ipc/**`, `src/types/generated/**`, `docs/contracts.md`. Change only with both developers' agreement; bump `PROTOCOL_VERSION` on breaking changes.
- Do not edit another developer's modules unless the task says so.
- Developer C's work is reviewed by A or B. C may be the second approver only for low-risk PRs in its own lanes (docs, scripts, CSS, tests), never approves or merges its own PR, and has one open PR per lane at a time. Anything touching permissions, approvals, the contract, `ipc/`, `capabilities/` or the entitlements waits for a review by Developer A (security paths stay with Claude, not with a model from C's family). `.github/CODEOWNERS` lists the paths. The approval and permission screens (`ApprovalCard`, `MemoryProposalCard`, `PolicySettings`, `WelcomeNotice`) are A and B only, and **a change to any `.approval-*`, `.memory-*` or permission rule in `src/styles.css` waits for Developer A**, because a file-level owner line cannot split a file (a restyle must not hide, shrink or recolour Deny, or make a card look like a normal message).

## Workflow between the two developers
- One session = one GitHub issue = one branch = one PR into `main`. Branch names: `a/<issue>-<slug>` or `b/<issue>-<slug>`.
- Start from the latest `main`. A cloud session clones GitHub, so anything not pushed does not exist for it.
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
- Speech engine is the official `sherpa-onnx` crate; do not add whisper.cpp, Piper or another runtime alongside it. The one exception is ONNX Runtime (the `ort` crate, loaded at run time) for the openWakeWord wake word only (D-29).

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
