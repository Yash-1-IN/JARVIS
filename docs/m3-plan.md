# M3 Voice: plan

Status: **proposed** (needs the owner's go-ahead, and kibong2's agreement on the contract in step 1).
Spec: §6 (voice pipeline), §10 (budgets), §13 (M3 acceptance). Evidence: `docs/spikes/s1-report.md`,
`docs/perf.md`. Decisions this plan relies on: D-21 (alpha targets the owner's Windows laptop and
kibong2's M2 Air), D-23 (openWakeWord for personal use only, files never committed or bundled), D-24
(the TTS voice cannot say its own name).

## Goal

Say "hey jarvis" (or press a key), speak a request, hear the answer. The voice pipeline drives the turn
loop that already exists (M2 text brain, M4 tools, local intents). No new assistant state machine: the
orchestrator keeps the only one, voice feeds it events.

## What exists already

- `MicState` (off, muted, standby, capturing, unavailable) and `AssistantPhase` (idle, listening,
  transcribing, thinking, awaiting_approval, acting, speaking, error) are already in the contract.
- The turn loop, local intents, cancel, tools and the gateway work from typed text.
- The voice spike (`spikes/voice-cli`) proved the parts in isolation: cpal capture and playback, Silero
  VAD, Moonshine and Whisper STT, Kokoro TTS (lexicon only), sherpa KWS, and openWakeWord through `ort`.
- Not in the app yet: `src-tauri/src/audio/` and `src-tauri/src/voice/` do not exist.

## Decisions to make first (small, mostly already leaning)

| ID | Question | Lean |
| --- | --- | --- |
| D-02 | Wake word engine | openWakeWord "hey jarvis", personal use only (D-23); phrase is "hey jarvis" only; threshold about 0.7 with two scores in a row; hotkey always available. Needs kibong2's Mac numbers. |
| D-04 | STT model | Moonshine base unless the S1 table says otherwise; record the final pick with the WER numbers. |
| D-03 | TTS | Kokoro v1.0 English voices (the owner picked sids 24 and 26 earlier), lexicon only, no espeak-ng. Confirm the list of voices that pass the audition. |
| D-05 | Echo | Alpha has no AEC. Four interrupt paths from §6.6; headphones recommended. `sonora` stays a post-alpha experiment. |
| new | `ort` and ONNX Runtime for openWakeWord | A dependency beside sherpa-onnx. CLAUDE.md allows one speech runtime; this needs its own decision (or the wake word goes through sherpa if it can load the model). |

## Steps (one issue, one branch, one PR each)

Each step ends with something that runs. Steps 1 and 2 are the only ones that touch shared files.

1. **Contract PR (shared, small).** Additive, `PROTOCOL_VERSION` stays 1 if possible. Proposed:
   - Commands: `voice_start_capture` (push-to-talk down), `voice_stop_capture` (up or cancel),
     `voice_status`.
   - `VoiceStatus { enabled, wake, micDevice, micPermission, modelsReady, error }`, never audio.
   - Events: `transcript_partial` and `transcript_final` (text only), `wake_detected`,
     `voice_models_progress`.
   - Settings (additive): `wakeEnabled`, `voice` (id), `inputDevice`, `followUpWindow`.
   - Reuse `ErrorCode`s: `MIC_PERMISSION_DENIED` exists in the spec; check it is in the enum before adding.
   Developer B reviews before anything builds on it.
2. **Model manager (A).** `models/manifest.json` (URL, size, SHA-256, licence, purpose), resumable
   download with checksum before use, "import from folder" for offline installs, app-data directory,
   default set at most 600 MB. Mock download source in tests. UI for progress and errors is B's (step 8).
3. **Audio capture (A).** `audio/`: cpal input → mono 16 kHz → lock-free ring buffer (nothing allocates or
   locks in the callback; the spike's `Vec` and channel are the shortcut not to copy). 2 s pre-roll in RAM
   only. Device hot-plug: loss → `mic: unavailable`, `phase: error`, retry every 2 s, recover without a
   restart. Mic permission detection on Windows and macOS. Never writes audio to disk (test that no file
   is created).
4. **Push-to-talk path first (A).** VAD end of speech (700 ms of silence, 15 s cap) → STT → the existing
   turn loop. A global hotkey and a UI button start it. This is the guaranteed input path and gives a
   working voice input before the wake word is decided. STT runs on a dedicated thread, never on a Tokio
   worker; every stage has a `CancellationToken` and a timeout. Models unload after 5 minutes idle.
5. **Speech output (A).** Kokoro through sherpa-onnx, sentence by sentence while the reply streams,
   first sentence plays as soon as it is ready. Output device selection. Interrupt: stop button, Esc,
   hotkey → silence within 300 ms. Voice text goes through the same markdown-stripping the UI uses so it
   never reads symbols aloud; names that G2P gets wrong go in a small pronunciation list (D-24).
6. **Wake word (A).** Runs on the voice thread while `mic: standby`. "hey jarvis" at 0.7 with two scores
   in a row (D-02). Raised threshold while TTS is playing so it can interrupt. Wake → visual
   acknowledgement within 400 ms. Wake word off by default until the owner turns it on; the pre-wake
   audio never leaves RAM (test: no outbound traffic and no files during standby).
7. **Follow-up window (A).** After speaking, listen 6 s without the wake word, with a visible indicator
   and a settings switch.
8. **Voice UI (B).** Settings → Voice (wake on/off, input and output device, voice, follow-up window),
   model download progress and errors, mic-permission guidance with a link to the OS setting, level
   indicator, live transcript line, mic and phase labels for every combination (text, not colour alone).
   Mock transport gets scripted voice runs with the MOCK badge. Info.plist
   `NSMicrophoneUsageDescription` and the macOS entitlement go in with this (kibong2's lane, `platform/`).
9. **Hardening and measurement (both).** The §10 budgets measured on the Windows laptop and the M2 Air
   and recorded in `docs/perf.md`; 8-hour soak (no audio files, no crash, memory flat); unplug and replug
   during standby and during capture; mic denied on both OSes; network capture in standby shows zero
   outbound traffic.

## Order and who

| Order | Step | Lead | Needs |
| --- | --- | --- | --- |
| 1 | Contract PR | A (B reviews) | D-02 direction |
| 2 | Model manager | A | contract |
| 3 | Audio capture | A | none (parallel with 2) |
| 4 | Push-to-talk path | A | 2, 3 |
| 5 | Speech output | A | 2 |
| 8 | Voice UI | B | contract (parallel with 2 to 7, on the mock) |
| 6 | Wake word | A | 3, 4, D-02 |
| 7 | Follow-up window | A | 5, 6 |
| 9 | Hardening | both | all |

Push-to-talk plus speech output (steps 1 to 5 and 8) is a usable voice assistant on its own. The wake
word is the last, riskiest piece, and it can slip without blocking the rest.

## Acceptance (spec §13, M3)

- [ ] Wake word and push-to-talk start a turn; the transcript appears; the reply is spoken in the chosen voice.
- [ ] §10 budgets for wake, transcript, first audio and interrupt are met on both reference machines.
- [ ] Unplug and replug the mic during standby and during capture: no crash, recovery without restart.
- [ ] Mic permission denied on macOS and Windows shows `MIC_PERMISSION_DENIED` with guidance.
- [ ] Network capture during standby shows zero outbound traffic from the app.
- [ ] No audio file is created anywhere during an 8-hour soak; no native crash.

## Risks

- **Wake word reliability.** The best measured result is 8 of 10 at the chosen threshold on one voice and
  one microphone, and false triggers are unproven below one per 4 hours. Push-to-talk is the fallback;
  the wake word ships off by default if the longer logs do not hold up.
- **Licence.** openWakeWord's pretrained model is non-commercial (D-23). This plan is for the two
  owners' use only; any wider release needs a custom-trained or commercial replacement (D-01, D-19).
- **No echo cancellation.** A speaker playing the reply can trigger the wake word or be heard as speech.
  Headphones are recommended; the raised threshold while speaking limits the damage.
- **First-sentence latency.** Kokoro on CPU took about 1.4 s for the first sentence in the spike. The
  GPU path is untested; the 1.5 s local-intent budget is tight.
- **Two runtimes.** sherpa-onnx and `ort` both load ONNX Runtime. Their versions must not clash inside
  one process; this is the first thing step 6 must prove on all three systems.
- **macOS permissions.** Ad-hoc signatures change on every build, so the mic prompt may repeat for
  release builds (spec §6.9); dev builds attach the permission to the terminal app.

## Not in M3

Echo cancellation, custom wake-word training, a local language model, speaker identification, and more
than one voice or language.
