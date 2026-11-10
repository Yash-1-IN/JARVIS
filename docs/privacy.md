# Privacy: what JARVIS stores and what leaves your computer

This describes the alpha as it is today. It is written for a person, not a lawyer. If something here is
wrong, that is a bug: please say so (see `SECURITY.md`).

## What leaves your computer

For its replies JARVIS talks to **one** service: the Anthropic API, using **your own API key**. There is no
JARVIS server, no account, no analytics and no crash reporting. Two other things are contacted, and only
when you choose them: **Spotify** (connecting it, and asking for a song or playlist, goes to
`accounts.spotify.com` and `api.spotify.com`, see below) and the **speech model download** (the
**Download speech models** button in Settings → Models fetches files from the release pages of the
sherpa-onnx project on `github.com`; if you turn the wake word on, also from the onnxruntime and
openWakeWord projects). `github.com` sees your internet address like any download; nothing you say is sent.

When you send a message, these go to Anthropic with it:

- the recent conversation (at most about 8,000 tokens of it);
- what a tool found or read for that request: file names, the **contents of a text file you approved it to
  read**, system status, the time, the list of apps you allowed;
- up to five of your **kept** memories that match your question (never ones still waiting for your answer);
- JARVIS's own instructions (its personality and the tool descriptions).

Anthropic's own terms and retention rules apply to what it receives. Check them before you send anything
you would not want them to hold.

Quick commands that never need the model (stop, mute, "repeat that", "what can you do", "remember that
...", and music controls such as "pause", "next song", "volume up" and "what's playing") are handled on your
computer. The music controls talk to your operating system's media player controls, not to the internet (on macOS, to the Spotify and Music apps through fixed scripts, after you allow it once under Privacy and Security, Automation); a
track name is read from the playing app and shown to you; like any reply, it can be part of the recent
conversation that is sent when you next message the model. "Play <words>" is the exception to "not the internet": the words you say go to Spotify (api.spotify.com) to find and play the song, nothing about it goes to the model provider, and a failure message can include the Spotify address of the chosen song (a public id, no search text), which appears in the chat and in history; the title Spotify returns is shown to you and, like any reply, may be part of the conversation sent with your next model message. For "play my ... playlist" JARVIS also reads the **names and addresses of your playlists** from Spotify (never what is inside them) to find the one you named; the names stay on your computer and may appear in the reply. The Spotify sign-in happens in your own browser; JARVIS keeps only a refresh token, in your system keychain, never in a file or in the app window, and Disconnect deletes it. When the spend cap is reached, there is no key, or you are offline, the
time, system status, open-an-app and find-a-file questions are also answered on your computer.

## What is stored on your computer

| What | Where (typical) | How long | Protected by |
| --- | --- | --- | --- |
| Your **API key** | The operating system's credential store (Windows Credential Manager, macOS Keychain; on Linux the desktop's own secret store). If none is available it is kept only until the app closes. | Until you remove it | The OS |
| **Settings** (model, spend caps, the most actions in one request, allowed folders and apps, the Spotify Client ID, and your voice choices: microphone, wake word on or off and whether its notice was accepted) | `settings.json` in the app config folder | Until you change it | OS file permissions |
| Your **Spotify sign-in** (a refresh token, only when you connect Spotify; never shown, never in a file) | The operating system's credential store, under its own name (`spotify-refresh-token`). If none is available it is kept only until the app closes. | Until you press Disconnect or change the Client ID | The OS |
| **Speech models** (only after you press Download; and the wake word's files if you turn it on) | `models` in the app data folder (about 400 MB on disk for speech recognition) | Until you delete the folder | OS file permissions |
| **Conversation history** (what you said, JARVIS's replies, one-line summaries of actions) | `jarvis.db` in the app data folder | **30 days by default**; you can choose off, 7 days, 1 year or forever | The OS disk encryption (see below) |
| **Memories** you kept, and ones waiting for your answer | `jarvis.db` | Until you delete them | The OS disk encryption |
| **Display preference** (3D hologram, 2D orb or automatic) | The window's local storage, inside the app data folder | Until you change it or clear the app's data | OS file permissions |
| **Exported file** (when you press Export) | A plain-text `jarvis-export-<date>.json` in your Downloads folder, saved by the window's own download | Until you delete it. It is **your file** after that: it is not covered by retention, "Delete all history" or Delete on a memory. | Whatever protects your Downloads folder |
| **Activity log** (every action JARVIS tried, who decided, how it ended; never file contents) | In memory only for now | Until the app closes | n/a |
| **Logs** (events and errors, with key-shaped text masked; not your messages. Voice lines record technical facts only: the microphone's name, how long a capture lasted, its peak level and how many words were recognised, never the sound or the words) | The app log folder | Rotated daily; the last 14 days are kept | OS file permissions |

Typical folders: Windows `%APPDATA%\com.jarvis.desktop` (data and settings) and
`%LOCALAPPDATA%\com.jarvis.desktop\logs`; macOS `~/Library/Application Support/com.jarvis.desktop` and
`~/Library/Logs/com.jarvis.desktop`; Linux `~/.local/share/com.jarvis.desktop` and
`~/.config/com.jarvis.desktop`. These come from the Tauri defaults and have not been checked on every
operating system yet.

**Never stored:** audio (the app does not record or write any audio), your API key anywhere except the OS
credential store, the **contents** of files JARVIS reads (history keeps a one-line summary such as "Read the
file X"), and anything that looks like a password or API key (it is masked before it is saved).

## You are in control

- **Nothing becomes a memory without you.** "Remember that ..." or a suggestion from JARVIS shows a card with
  the exact text. Only **Keep** makes it usable. **Don't keep** or **Delete** removes it for good, including
  from the search index and from the history record of the request (history keeps only "Asked JARVIS to
  remember something").
- **History**: choose how long to keep it, or turn it off, in the History panel. **Delete all history**
  removes every conversation (memories stay) and stops a reply in progress.
- **Export** gives you one JSON file of your memories and history.
- **Allowed folders and apps** limit what JARVIS may touch. It asks before it reads or moves your files,
  unless you switch that off for routine actions in Settings → Permissions. Even then it asks again after it
  has read anything from outside, and always for bigger actions such as moving a whole folder.
- **Spend caps** stop new requests when your daily or monthly limit is reached.

## Honest limits

- The database is **not encrypted by JARVIS**. We rely on your operating system's disk encryption
  (BitLocker or Device Encryption on Windows, FileVault on macOS, LUKS or similar on Linux). Without disk
  encryption, anyone with access to your files can read your history and memories. (Decision D-10.)
- With history set to **off**, JARVIS also does not start a fresh conversation after a pause.
- Deleted text is overwritten in the database file and its write-ahead log, but file systems and backups
  outside JARVIS (Time Machine, cloud sync, file-history features) may keep older copies.
- A file JARVIS reads is sent to Anthropic. Only allow folders that you are happy for it to read from.
- The Activity log is not saved to disk yet, so it is empty after a restart.
