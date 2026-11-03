# Known limitations (alpha)

What does not work yet, and what to expect. Updated with the code; if you find something not listed, tell us.

## Not built yet
- **Voice**: no wake word, speech-to-text or spoken replies. Typing only.
- **Apps and files**: it can open apps you allowed, but it cannot close them or pass them files. It reads **plain
  text** files only (not Word, PDF or spreadsheets), up to 1 MB. It cannot delete files, and never overwrites
  one when moving or renaming.
- **Music**: pause, play, next, previous, volume, mute and "what's playing" work on **Windows and macOS** (Linux says they are not available). On Windows they act on whatever Windows treats as the current media session, so if a browser tab played sound most recently, that is what pauses. **On macOS they reach the Spotify and Music apps only** (not browsers or other players): macOS has no public "current media session". The first time, macOS asks you to let JARVIS control the app (System Settings, Privacy and Security, Automation); if you said no, switch it on there. A player that is not already open is never started. If both are open, the one that is playing is used. These controls cannot choose a song; "play <words>" does that through Spotify (next item). Volume moves about 10% per command, and "mute the music" toggles.
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
- **Play a song (Spotify, music step 2)**: "play <words>" searches Spotify and plays the best match on your Spotify app. It needs a one-time setup under Settings, Music: your own free Spotify developer app (its Redirect URI is `http://127.0.0.1:43597/callback`) and your Spotify **Premium**. The Spotify app should be open, or at least listed in your devices. **On Windows (tested on one laptop with the Spotify app from spotify.com), if Spotify is installed but closed, JARVIS starts it and waits up to about 12 seconds for it to show up before playing; with it open, a song starts in about a second or two** (it never opens a dialog when Spotify is not installed; it says no device is open instead). It plays the single best search match, so a vague request may play the wrong song; it cannot pick an album track by track, make playlists or like songs yet. Spotify's development-mode rules limit an app to a few listed users. The sign-in lasts until you press Disconnect or Spotify revokes it. After a song starts, the conversation counts as containing outside text (the title comes from Spotify).
