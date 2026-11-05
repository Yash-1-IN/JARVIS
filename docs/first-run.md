# First-run guide (alpha)

Status: **not tested on a second computer** (waiting for Developer A's owner to follow this guide on their Windows laptop and confirm in the PR).

This guide walks you through your first few minutes after opening JARVIS. It is written in plain language for someone who is not a developer.

## What works today

- **Typing, or your voice**: you can type in the **Type a message** box and press **Send**, or press **Hold or tap to talk** and speak. Before your first spoken request, open **Settings → Models** and press **Download speech models** (about 250 MB, once). JARVIS answers in text: **spoken replies are not built yet**, and the wake word ("hey jarvis") cannot be turned on from the window yet.
- **One decision on first launch**: the first time JARVIS opens, it shows a box titled **One decision before we begin**. By default, JARVIS shows an approval card and waits for you before reading, moving, or renaming any file.
  - Click **Keep asking** (or **Close Esc**) to keep those approval cards.
  - Click **Stop asking for routine actions** if you want JARVIS to read, move, or rename single files inside your allowed folders without showing a card first (reading a file sends its contents to Anthropic). Even with this switched on, JARVIS still asks after it has read outside content in the same conversation, and always asks before high-risk actions.
  - You can change this setting at any time in **Settings** → **Permissions** (`Let medium-risk actions (reading, moving or renaming a single file) run without asking`).

## 1. Add your Anthropic API key

JARVIS uses your own Anthropic API key to answer general questions. Without a key, local commands still work, but general chat replies will show **API key problem: No API key is set.**

1. Click **Settings** in the top navigation bar.
2. In the **Key and model** tab (selected by default), find **Anthropic API key**.
3. Paste your key into the **API key** password box and click **Save key**.
4. JARVIS checks the key once with Anthropic, then stores it in your operating system's credential store (Windows Credential Manager on Windows, macOS Keychain on macOS, or the desktop secret store on Linux).
   - If your system has no credential store available, JARVIS keeps the key in memory only until the app closes and shows a warning.
   - JARVIS never writes your API key to a settings file or log, never shows the full key again (only the ending hint), and never sends it anywhere except to Anthropic.
   - You can replace the key later using **Replace key** and **Save key**, or delete it with **Remove key**.
5. Under **Model**, you can keep the default model (`claude-haiku-5-5`, the lowest-cost option) or pick another model from the list (`claude-sonnet-5-5`, `claude-opus-5-5`, `claude-fable-5-1`).

## 2. First typed commands to try (no cloud model needed)

Type any of these into the **Type a message** box at the bottom of the window and click **Send** (or press Enter).

### Commands that always run on your computer (free and instant)

These commands never call the Anthropic cloud model, whether or not you have an API key or internet connection:

- **`What can you do?`** — replies with a short summary of what JARVIS can do today.
- **`mute`** and **`unmute`** — mutes or unmutes the microphone state (`Muted.` / `Microphone back on.`). You can also click the **Mute Ctrl+Shift+M** button in the controls area (a muted microphone cannot be used for **Hold or tap to talk**).
- **`stop`** or **`cancel`** — cancels a reply that is currently running. You can also click **Cancel Esc** or press `Esc`.
- **`Repeat that`** or **`Say that again`** — repeats JARVIS's last reply.
- **`Remember that ...`** (for example, `Remember that I prefer morning meetings.`) — shows a **Remember this?** confirmation card with the exact text and three buttons: **Keep**, **Edit**, and **Don't keep**. Nothing is saved until you click **Keep** (or **Edit** → **Save and keep**). You can view, edit, or delete saved memories at any time under **Memory** in the top bar.
- **Music controls**:
  - Try typing **`pause`**, **`play`** (or **`resume`**), **`next song`**, **`previous song`**, **`volume up`**, **`volume down`**, **`mute the music`**, or **`what's playing`**.
  - These talk to your operating system's media controls, not to the internet. On **Windows**, they control whatever Windows treats as the current media session (so if a browser tab played sound most recently, that is what pauses).
  - Volume moves about 10% per command, and `mute the music` toggles system audio mute (plain `mute` is for the microphone). After `what's playing`, the track name comes from another app, so the conversation counts as containing outside text and routine file actions ask for approval again even if routine prompts are switched off.
  - Playing music by name (`play <words>`) needs a one-time Spotify setup; see [`docs/known-limitations.md`](known-limitations.md).

### Commands that use local tools directly when offline, out of budget, or without an API key

When you have an API key and internet access, the cloud model can call these tools for you. When you have **no API key**, **no internet connection**, or have **reached your spend cap**, JARVIS answers these exact questions on your computer directly without the model:

- **`What time is it?`** or **`What's the date?`** — tells you the current date and time. (Today JARVIS knows UTC only and does not know your local time zone on its own.)
- **`How's my battery?`** or **`System status`** — checks your computer's battery and system status.
- **`Open <app name>`** (for example, `Open Calculator`) — opens an application, **only if you ticked it first** in **Settings** → **Permissions** under **Allowed apps**. If the app is not ticked, JARVIS tells you to allow it in Settings. JARVIS can open allowed apps, but cannot close them or pass files to them.
- **`Find my <name>`** (for example, `Find my notes`) — searches by file name inside the folders you added in **Settings** → **Permissions** under **Allowed folders** (use **Choose folder…** or type a full path and click **Add folder**).

When the cloud model asks to read a plain-text file (up to 1 MB), move a file, or rename a file inside your allowed folders, JARVIS shows an **Approve this action?** card with **Approve** and **Deny** buttons and a 60-second timer (`60s left`), unless you switched routine prompts off. JARVIS cannot read Word, PDF, or spreadsheet files, cannot delete files, and never overwrites an existing file when moving or renaming.

## 3. What happens with no internet connection

- When JARVIS first starts, the status line under the orb shows **Network unknown**. JARVIS only detects that you are offline when a cloud request fails.
- If you send a general question while disconnected from the internet, the request fails without adding the failed turn to your history. The status line changes to **Offline: local features only**, and an error banner shows **You're offline** ("Local commands still work; cloud answers resume when you reconnect.") with a **Retry** button.
- While offline, all of the local commands and local tool questions listed in Section 2 (`mute`, `stop`, `what can you do?`, `repeat that`, `remember that ...`, system music controls such as `pause` and `next song`, `what time is it?`, `how's my battery?`, `open <allowed app>`, and `find my <file>`) continue to work on your computer.
- Free-form questions and multi-step requests need the cloud model and wait until you are back online.

## 4. Where usage and the spend cap are shown

Every cloud reply uses your Anthropic API key and costs money on your Anthropic account. You can check your usage and set spending limits inside JARVIS:

1. Click **Settings** in the top bar, then click the **Spending** tab.
2. Under **Spend caps**, enter a dollar amount in **Daily cap (USD)** and/or **Monthly cap (USD)** (for example, `2` or `2.50`, or leave blank for `No cap`) and click **Save caps**.
3. Under **Usage**, JARVIS shows:
   - **Today**: how much you have spent today, your daily cap, and when the daily counter resets.
   - **This month**: how much you have spent this month, your monthly cap, and when the monthly counter resets.
   - **Tokens today**: input (`in`) and output (`out`) tokens used today.
4. Daily and monthly counters reset at UTC midnight and the start of the UTC month, and the reset time is displayed in your local time (decision D-15).
5. When you reach a daily or monthly cap, JARVIS blocks new cloud requests and shows **Spend cap reached**, while local commands keep working. One long reply already in progress can go slightly over the cap, and cancelled or interrupted replies are counted at the full 4,096-token output limit so the spend cap stays on the safe side.

## 5. What leaves your computer (and what stays on it)

This matches [`docs/privacy.md`](privacy.md), which describes everything JARVIS stores and sends.

### What leaves your computer

- For its replies JARVIS talks to **one** cloud AI service: the **Anthropic API**, using your own API key. There is **no** JARVIS server, no account, no analytics, and no crash reporting.
- Two other things are contacted, only when you choose them: **Spotify** (when you connect it or ask for music by name: `accounts.spotify.com` and `api.spotify.com`) and the **speech model download** (the **Download speech models** button fetches files from `github.com`, which sees your internet address like any download; nothing you say is sent).
- While a cloud request is running, the status area displays **Sending data to the cloud**.
- When you send a message that goes to the Anthropic cloud model, JARVIS sends:
  - the recent conversation (at most about 8,000 tokens of it);
  - anything a tool found or read for that request: file names, the **contents of a plain-text file you approved JARVIS to read**, system status, the time, and the list of apps you allowed;
  - up to five of your **kept** memories that match your question (never memories still waiting on a confirmation card);
  - JARVIS's own system instructions (its personality and tool descriptions).
- Anthropic's own terms and data-retention policies apply to what you send them. Only allow folders and approve file reads for files you are comfortable sending to Anthropic.

### What stays on your computer

- **Your API key**: stored in your operating system's credential store (Windows Credential Manager on Windows, macOS Keychain on macOS, or the desktop secret store on Linux).
- **Settings** (model, spend caps, the most actions in one request, allowed folders, allowed apps, the Spotify Client ID and your voice choices): stored in `settings.json` in the app configuration folder.
- **Your Spotify sign-in** (only if you connect Spotify): a refresh token kept in the operating system's credential store until you press **Disconnect** or change the Client ID.
- **Speech models** (only after you press **Download speech models**): stored in the `models` folder inside the app data folder, about 250 MB.
- **Conversation history**: stored in `jarvis.db` in the app data folder and kept for **30 days by default**. In the **History** panel (top bar), you can change **Keep history for** to `Off (keep nothing)`, `7 days`, `30 days`, `1 year`, or `Forever`, or click **Delete all history**. A new conversation starts automatically after ten idle minutes (there is no "new conversation" button yet).
- **Memories**: stored in `jarvis.db` only when you click **Keep** on a **Remember this?** card, and kept until you delete them in the **Memory** panel.
- **Exported data**: if you click **Export memories and history** in the **History** panel, JARVIS saves a plain-text `jarvis-export-<date>.json` file to your Downloads folder. Once exported, that file is yours to manage and is not deleted by **Delete all history**.
- **Activity log**: clicking **Activity** in the top bar shows every tool action JARVIS ran or refused during the current session (never file contents). It is kept in memory only and is empty after you restart JARVIS.
- **Logs**: event and error logs (not your chat messages, and with key-shaped text masked) are rotated daily in the app log folder, and the last 14 days are kept. Voice lines record only technical facts: the microphone's name, how long a capture lasted, its peak level and how many words were recognised, never the sound or the words.
- **Typical Windows folders** (from Tauri defaults; not checked in the app on every system): `%APPDATA%\com.jarvis.desktop` for data and settings, and `%LOCALAPPDATA%\com.jarvis.desktop\logs` for logs.
- **Disk encryption note**: `jarvis.db` is not encrypted by JARVIS itself (decision D-10). It relies on your operating system's disk encryption (BitLocker or Device Encryption on Windows).
- **Never written to disk**: audio is never recorded or saved to disk, your API key is never written outside the OS credential store, and the contents of files JARVIS reads are never saved in `jarvis.db` (history keeps only a one-line summary of the action).
