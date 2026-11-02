# S1 voice spike report (task A-10) — draft, in progress

Status: **partial**. Gate G1 is not decided. This file records measurements as they arrive;
nothing here is a decision (decisions go in `docs/decisions.md`: D-02, D-03, D-04, D-05, D-20).

Tool: `spikes/voice-cli` (task A-08). Engine: `sherpa-onnx` 2.0.1 with `cpal`.
Budgets: spec §10.

## Machines

| Label | Machine | OS | Mic | Notes |
| --- | --- | --- | --- | --- |
| W1 | Owner's Windows laptop (model, CPU and RAM to be added) | Windows | Realtek array mic, 48 kHz, 2 ch | Not yet checked against the §10 reference machine (4 cores / 8 threads, 8 GB) |
| C1 | Cloud VM, Intel Xeon 2.1 GHz | Linux | none | Sample WAVs only; not a reference machine |

## Capture (W1)

| Check | Result |
| --- | --- |
| `mic --seconds 5` | 99.9% of the expected 16 kHz samples, 500 callbacks, 0 stream errors, level rms 0.0093 / peak 0.066 (quiet but usable) |

## Speech to text: Moonshine tiny (English, int8, MIT per spec) + Silero VAD

| Machine | Input | STT time | Real-time factor | Notes |
| --- | --- | --- | --- | --- |
| W1 | 6.6 s sample clip | 130 ms | 0.02 | Correct text; models load in 1.1 s |
| C1 | 6.6 s sample clip | 158 ms | 0.02 | Correct text |
| W1 | live, 3.6 s and 4.3 s of speech | 59 to 61 ms | about 0.015 | Teacher's voice across a noisy classroom: words wrong, which is the test conditions, not a verdict on the model |

End of speech to text on W1: about 760 ms, of which 700 ms is the VAD's silence window (spec §6.2
default) and about 60 ms is the model. Against §10 "End of speech → transcript (5 s utterance)":
hard limit 1.2 s **met**, target 0.5 s **not met with a 700 ms window**; it would be met with a
window near 400 ms, at the cost of cutting off slow speakers. To decide with data.

Standby cost while listening (VAD + STT, one thread): 1.8% of wall time. Not the Task Manager
figure, and the wake word was not running.

Open: WER on 100 command utterances in a quiet room, at 0.5 m and 1 m (needs the corpus, task A-09).
Moonshine vs Whisper base.en not compared yet.

## Wake word (D-02): sherpa-onnx open-vocabulary KWS (zipformer 3.3M, GigaSpeech)

| Check | Result |
| --- | --- |
| Detects a test phrase in a sample WAV (C1) | Yes ("light up", 7.1 s of audio decoded in 192 ms) |
| "hey jarvis" false accepts per hour, false rejects at 1 m and 3 m | **Not measured** |
| CPU and RAM in standby on W1 | **Not measured** |

Phrase file uses the model's own BPE pieces: `▁HE Y ▁JA R VI S @hey_jarvis`.
Model licence: card says Apache-2.0; trained on GigaSpeech data. Check before shipping (D-20).

## Text to speech (D-03): Kokoro-82M int8 in lexicon-only mode

| Check | Result |
| --- | --- |
| Loads and speaks without espeak-ng (sherpa-onnx 2.x) | Yes (C1) |
| Real-time factor | 1.1 to 1.7 on C1 (2.1 GHz Xeon). **Not measured on W1** |
| Time to first sound, first sentence | 1.7 to 2.6 s on C1. **Not measured on W1** |
| Which speaker id is the British male voice (`bm_george`/`bm_lewis`) | **Not measured** (103 ids in this build; mapping differs between Kokoro versions) |
| Out-of-vocabulary names in lexicon-only mode | **Not measured** (names such as "Mumbai" rendered, quality not judged) |
| Blind audition, 5 listeners | **Not done** |

## Echo and interruption (D-05)

Not started. The spike has no echo cancellation: test `say` and `listen` apart, or with headphones.

## Gate G1

Not decided. Missing: wake-word FA/FR, W1 TTS speed and voice choice, quiet-room WER, a second
machine (Mac numbers are expected from the other developer), the §10 reference-machine spec.
