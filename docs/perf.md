# Performance record

What has been **measured** against the budgets in spec §10, and what has not. Hard limits block the
alpha; targets are what we tune toward. Nothing here is a guess: a number appears only with where it came
from. Rows marked "not measured" stay that way until someone measures them.

## Machines

Per decision D-21 the alpha targets two machines instead of the spec's 8 GB reference laptop. The §10
budgets are still the yardstick, but they are advisory on these machines.

| Label | Machine | Notes |
| --- | --- | --- |
| W1 | Owner's Windows laptop: ASUS ROG Strix G16 (G614JI), device name LAPTOP-JSVPQ189 | 13th Gen Intel Core i7-13650HX (2.60 GHz; 14 cores and 20 threads per Intel's published spec), 16 GB RAM (15.6 GB usable, 4800 MT/s), NVIDIA GeForce RTX 4070 Laptop GPU (8 GB) and Intel UHD Graphics (128 MB), 1.40 TB storage. Realtek microphone array at 48 kHz stereo. The Windows version is still to be added. |
| M1 | Developer B's MacBook Air M2, 16 GB | No numbers yet. |
| C1 | Cloud VM, Intel Xeon 2.1 GHz, 4 cores, Linux | Not a target machine. Used only to check that tools work. |

W1 is far faster than the spec's reference machine (4 cores, 8 GB, integrated graphics), so a budget that is
met on W1 says little about that machine; D-21 makes W1 and M1 the targets, so that is accepted. A budget
that is missed on W1 would be a real problem.

## §10 budgets and what we know

"Source" points to the section of `docs/spikes/s1-report.md` (the voice spike) where the run is described.

| Metric | Hard limit | Target | Measured so far | Status |
| --- | --- | --- | --- | --- |
| CPU, standby, wake word on | ≤ 5% (10-min average) | ≤ 2% | Listening loop only (no window, no app): 1.3 to 1.6% of one thread for the general wake-word model (W1). Task Manager figure for the whole app: not measured. | Partial |
| CPU, window visible, hologram idle | ≤ 8% | ≤ 4% | Not measured | Open |
| Resident memory, standby | ≤ 400 MB | ≤ 250 MB | The wake-word test program alone stayed at 42 to 43 MB (W1). The app with models loaded: not measured. | Partial |
| Peak memory during a turn | ≤ 1.2 GB | ≤ 800 MB | Not measured | Open |
| Wake detected → visual acknowledgement | ≤ 400 ms | ≤ 200 ms | Not built | Open |
| End of speech → transcript (5 s utterance) | ≤ 1.2 s | ≤ 0.5 s | About 0.74 s (Moonshine tiny), 0.78 s (Moonshine base), 0.94 s (Whisper base.en), W1. 0.7 s of each is the silence window; the model itself takes 41, 76 and 241 ms per phrase. The hard limit is met; the target needs a window near 0.4 s. | Hard limit met |
| End of speech → first audio, local intent | ≤ 1.5 s | ≤ 0.8 s | **Estimate, not an end-to-end run:** about 0.78 s (transcript) + 1.4 s (first voice sentence, Kokoro on 4 CPU threads) + 0.2 s (speaker start) = about 2.4 s on W1. Typed local commands, such as "pause" or "what time is it", skip the voice entirely and answer at once. | Not met for voice |
| End of speech → first audio, cloud, short question (p50) | ≤ 3.5 s | ≤ 2.0 s | Not measured | Open |
| Interrupt → silence | ≤ 300 ms | ≤ 150 ms | Cancelling a streamed text reply: tested at under 300 ms against a fake server (unit test). Voice interrupt: not built. | Partial |
| Hologram frame rate while animating | ≥ 30 fps | 60 fps | Not measured | Open |
| Hologram when idle or hidden | 0 frames | — | Not measured | Open |
| Cold start → interactive | ≤ 4 s | ≤ 2 s | Not measured | Open |
| Installer size (no models) | ≤ 50 MB | ≤ 25 MB | Built by CI for all three systems; size not recorded here yet | Open |
| Default model set on disk | ≤ 600 MB | ≤ 400 MB | Candidates (int8): Moonshine tiny about 120 MB, Moonshine base about 250 MB, Whisper base.en about 210 MB, Kokoro v1.0 about 130 MB, wake word 3.3M about 20 MB. A default set is not chosen. | Open |
| Wake word false accepts | ≤ 1 per 4 h | ≤ 1 per 24 h | General model: 0 in 5 minutes of background (W1), which proves nothing about a rate of one per 4 hours. | Open |
| Wake word false rejects (1 to 3 m, quiet room) | ≤ 10% | ≤ 5% | General model, W1, 20 takes: **about 35% (hard limit not met).** openWakeWord "hey jarvis": not measured yet. | Not met |
| STT word error rate on team corpus | ≤ 12% | ≤ 7% | 30 command phrases, one speaker, W1: 6.6% (Moonshine tiny), 2.9% (Moonshine base), 2.2% (Whisper base.en). Not the team corpus. | Indicative |

## Other numbers recorded along the way

- Microphone capture (W1, laptop array): 99.9% of the expected samples, no stream errors.
- The voice detector clips the first sounds of a request: 3 more phrases wrong out of 30 for every model,
  recovered by padding the cut with 0.3 to 0.6 s of earlier audio (the spec's pre-roll).
- Kokoro speaks 1.0 to 1.1 times slower than playback on 4 CPU threads, and 6 or 8 threads are not faster.
  Running it on the GPU (`--provider`) is untested.
- Windows makes Bluetooth earbuds the default microphone when they connect; with them, the wake word heard
  nothing (0 of 10).

## How to add a number

Run the command in `spikes/voice-cli/README.md`, or a scripted measurement for an app metric, paste the
result into the S1 report, then update the row here with the machine label and what exactly was measured.
