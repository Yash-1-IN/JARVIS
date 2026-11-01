# Privacy: what JARVIS stores and what leaves your computer

This describes the alpha as it is today. It is written for a person, not a lawyer. If something here is
wrong, that is a bug: please say so (see `SECURITY.md`).

## What leaves your computer

JARVIS talks to **one** service: the Anthropic API, using **your own API key**. There is no JARVIS server,
no account, no analytics and no crash reporting. Nothing else is contacted.

When you send a message, these go to Anthropic with it:

- the recent conversation (at most about 8,000 tokens of it);
- what a tool found or read for that request: file names, the **contents of a text file you approved it to
  read**, system status, the time, the list of apps you allowed;
- up to five of your **kept** memories that match your question (never ones still waiting for your answer);
- JARVIS's own instructions (its personality and the tool descriptions).

Anthropic's own terms and retention rules apply to what it receives. Check them before you send anything
you would not want them to hold.

Quick commands that never need the model (stop, mute, "repeat that", "what can you do", and "remember that
...") are handled on your computer. When the spend cap is reached, there is no key, or you are offline, the
time, system status, open-an-app and find-a-file questions are also answered on your computer.

## What is stored on your computer

| What | Where (typical) | How long | Protected by |
| --- | --- | --- | --- |
| Your **API key** | The operating system's credential store (Windows Credential Manager, macOS Keychain; on Linux the desktop's own secret store). If none is available it is kept only until the app closes. | Until you remove it | The OS |
| **Settings** (model, spend caps, allowed folders and apps) | `settings.json` in the app config folder | Until you change it | OS file permissions |
| **Conversation history** (what you said, JARVIS's replies, one-line summaries of actions) | `jarvis.db` in the app data folder | **30 days by default**; you can choose off, 7 days, 1 year or forever | The OS disk encryption (see below) |
| **Memories** you kept, and ones waiting for your answer | `jarvis.db` | Until you delete them | The OS disk encryption |
| **Display preference** (3D hologram, 2D orb or automatic) | The window's local storage, inside the app data folder | Until you change it or clear the app's data | OS file permissions |
| **Exported file** (when you press Export) | A plain-text `jarvis-export-<date>.json` in your Downloads folder, saved by the window's own download | Until you delete it. It is **your file** after that: it is not covered by retention, "Delete all history" or Delete on a memory. | Whatever protects your Downloads folder |
| **Activity log** (every action JARVIS tried, who decided, how it ended; never file contents) | In memory only for now | Until the app closes | n/a |
| **Logs** (events and errors, with key-shaped text masked; not your messages) | The app log folder | Rotated daily | OS file permissions |

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
