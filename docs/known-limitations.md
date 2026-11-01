# Known limitations (alpha)

What does not work yet, and what to expect. Updated with the code; if you find something not listed, tell us.

## Not built yet
- **Voice**: no wake word, speech-to-text or spoken replies. Typing only.
- **Apps and files**: it can open apps you allowed, but it cannot close them or pass them files. It reads **plain
  text** files only (not Word, PDF or spreadsheets), up to 1 MB. It cannot delete files, and never overwrites
  one when moving or renaming.
- **Time**: it knows UTC and does not know your time zone on its own.
- **Network state**: the status area shows "Network unknown"; it is only detected when a request fails.
- **Activity log** is kept in memory and is empty after a restart.
- **Memory** matches words, not meaning (no embeddings), so a differently worded question may not find a note.
- **A new conversation** cannot be started with a button; it starts after ten idle minutes.

## Platforms
- **Windows** is the most tested. It is unsigned, so SmartScreen warns on install (More info, Run anyway).
- **macOS** builds are ad-hoc signed and not notarised, so it must be opened with right-click, Open (or
  System Settings, Privacy and Security, Open Anyway). The build is for Apple silicon only. Checks on a real
  Mac are still in progress (issue #14), including the History export download.
- **Linux** builds exist and the tests run on it in CI, but the app has never been run there with a display.

## Behaviour to expect
- A reply costs money on your own key; the default model is the cheapest. Set a spend cap.
- The model can be wrong or terse. Every action it takes is checked by JARVIS's permission layer, but the
  permission layer cannot judge whether an action was a good idea: read the approval cards.
- Cancelled replies are charged at the full output limit, to keep the spend cap on the safe side.
- No auto-update yet: install a newer build by hand.
- The app uses the Tauri default icon until proper artwork exists.
