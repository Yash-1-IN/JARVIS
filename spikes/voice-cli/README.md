# voice-cli — spike A-08

Throwaway terminal program that tries the whole voice chain on real hardware and prints
timings: microphone → wake word → voice-activity detection → speech-to-text → text-to-speech.
It is **not** the app and nothing in `src-tauri/` uses it. It writes no audio to disk (it only
reads WAV files you give it). Spec: `docs/spec.md` §6 and §10, task A-08; results go into the
S1 report (A-10).

Built on `sherpa-onnx` (one engine for all four stages, Apache-2.0) and `cpal` (audio I/O).
Models are **not** in the repo: you download them below. Check each model's licence before it
goes anywhere near the product (D-01).

## Get the program

Easiest: open the **spike-voice** workflow run on the PR (or run it with "Run workflow"),
download the artifact for your OS (`voice-cli-Windows`, `voice-cli-macOS`, `voice-cli-Linux`).
Or build it: `cd spikes/voice-cli && cargo build --release` (needs Rust; Linux also needs
`libasound2-dev`).

## Get the models (Windows PowerShell; macOS/Linux: same URLs with `curl -LO` and `tar xjf`)

```powershell
mkdir models; cd models
$b = "https://github.com/k2-fsa/sherpa-onnx/releases/download"
curl.exe -LO "$b/asr-models/silero_vad.onnx"
curl.exe -LO "$b/asr-models/sherpa-onnx-moonshine-tiny-en-int8.tar.bz2"
curl.exe -LO "$b/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2"
curl.exe -LO "$b/tts-models/kokoro-int8-multi-lang-v1_1.tar.bz2"
Get-ChildItem *.bz2 | ForEach-Object { tar xjf $_.Name }
```

Kokoro: make a folder with only `model.int8.onnx`, `voices.bin`, `tokens.txt` and the
`lexicon-*.txt` files (copy them, leave the rest). The `espeak-ng-data` folder is GPL-3.0 and is
not needed by sherpa-onnx 2.x, so keep it out.

Wake-word phrase file (`keywords.txt`, one line per phrase, tokens are the model's own BPE pieces):

```
▁HE Y ▁JA R VI S @hey_jarvis
▁JA R VI S @jarvis
```

## Run it

```powershell
.\voice-cli.exe devices
.\voice-cli.exe mic --seconds 5
.\voice-cli.exe transcribe --wav models\sherpa-onnx-moonshine-tiny-en-int8\test_wavs\0.wav `
  --vad models\silero_vad.onnx --asr moonshine --asr-dir models\sherpa-onnx-moonshine-tiny-en-int8
.\voice-cli.exe listen --vad models\silero_vad.onnx --asr moonshine `
  --asr-dir models\sherpa-onnx-moonshine-tiny-en-int8 --seconds 60
.\voice-cli.exe wake --kws-dir models\sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01 `
  --keywords keywords.txt --seconds 120
.\voice-cli.exe say --tts-dir models\kokoro --sid 24,25,26,27 `
  --text "Good evening, sir. Shall I open Chrome?"
```

## What to paste back (issue #14 or the S1 report)

1. Your laptop model, CPU, RAM, Windows version, microphone and speakers.
2. `mic --seconds 5`: the "% of the expected count", stream errors and level lines. Peak 0 means
   Windows privacy blocks the mic (Settings → Privacy → Microphone → desktop apps).
3. `listen` while you say 5 short requests: the STT time and "text ready … after end of speech".
4. `wake` for 2 minutes **silent** (false triggers), then saying "Jarvis" / "hey Jarvis" 10 times
   at normal and far distance (hits out of 10). Note the "busy %" line and, in Task Manager, the
   program's CPU % and memory.
5. `say`: the time to first sound and the real-time factor per voice id; which id sounds like
   the British male voice (the ids differ between Kokoro versions).
6. Anything that crashed or sounded wrong.

## What this spike does NOT do

- No echo cancellation (D-05): run `say` and `listen` apart, or use headphones.
- The microphone callback allocates and uses a std channel; product code needs a lock-free ring.
- Linear resampling, one sentence at a time, no streaming TTS, no pre-roll buffer.
- "busy %" counts one thread; the Task Manager figure is the one that matters.
