# Threat model (alpha)

The controls and trust boundaries are in `docs/spec.md` §9. This file records what the
permission gateway and the path scope do **not** defend against yet, so nobody assumes more
than they get.

## Known limits

- **Check-then-use window.** `permissions::paths` checks a path (canonical, inside an allowed
  root) and the tool opens or renames it a moment later. The gateway re-checks right before
  execution, which catches a file swapped for a symlink after approval, but a swap inside that
  last window is not prevented. Closing it needs `openat` with `O_NOFOLLOW` (Unix) or
  handle-based moves (Windows).
- **Hard links.** A hard link inside an allowed folder that points to a file outside it passes
  canonicalisation, and hard links cannot be detected portably. An allowed folder should not
  contain links the user does not trust.
- **Case sensitivity is detected per allowed folder** by asking the file system (the same
  name with its case swapped must resolve to the same folder). A case-sensitive volume mounted
  *inside* a case-insensitive root is not detected.
- **Windows names.** Destination names are checked for alternate data streams (`:`), reserved
  device names (`CON`, `NUL`, `COM1`...), trailing dots or spaces and characters Windows
  forbids. Short (8.3) names are not specially handled.
- **Approvals are single-use even when answered wrongly.** A mismatched or late answer burns
  the approval and the model has to propose again. This is deliberate (fail closed).
- **The activity log is in memory** until M5 adds SQLite, so it is lost when the app quits.
- **Voice turns (D-27).** Speech is not authenticated: a voice from a TV or another person can say
  anything. On a turn that starts from the microphone, a Medium-risk action always shows its approval
  card, even when routine actions are allowed without one, and High-risk actions are window-only. Low-risk
  tools (time, system status, listing allowed apps, opening an allowed app, searching file names,
  proposing a memory, music) still run without a card.
- **The wake word (D-29).** While it is on, the microphone stays open and a spoken phrase can start a
  turn, with the same limits as above. The three openWakeWord models are loaded from the exact bytes that
  were checked, but the downloaded ONNX Runtime library has to be opened by path, so a process running as
  the same user could swap that file between the check and the load in the app data folder. That
  attacker already owns the account, so it is accepted and recorded in D-29.
