# macOS setup guide (alpha)

Status: **not tested on a second Mac** (named tester: kibong2 on the M2 Air; marked not tested on a second Mac until a person other than kibong2 follows it on a Mac that did not build the app).

This guide explains how to install and open JARVIS on macOS (Apple silicon only). It is written in plain language for someone who is not a software developer. If you are a developer and want to run JARVIS from source, see the section at the bottom.

## What to expect in the alpha

- **Voice input & typing**: push to talk is available—you can type, or press the **Hold or tap to talk** button in the window and speak (hold it while you speak, or tap it and stop talking). Speech to text runs on your Mac. Spoken replies are not built. The **wake word ("hey jarvis") is not offered on a Mac** (it works on Windows and Linux), so the button is how you speak here. Speech needs model files, which you download once under **Settings → Models** (the size is shown there); they are verified against fixed checksums. See [`docs/known-limitations.md`](known-limitations.md).
- **Apple silicon only**: the current alpha DMG build is compiled for Apple silicon Macs.
- **No automatic updates**: when a newer build is ready, download and install the new DMG build by hand.
- **Default icon**: the app uses the default Tauri icon until custom artwork exists.

## 1. Download the macOS installer

JARVIS does not publish public release pages yet. Instead, GitHub builds the macOS DMG automatically under the `installers` workflow (`.github/workflows/installers.yml`).

1. Open the repository on GitHub and click **Actions** (not checked on a Mac).
2. In the left sidebar, click the **installers** workflow (not checked on a Mac).
3. Open the latest successful workflow run and scroll down to **Artifacts** (not checked on a Mac).
4. Download **jarvis-macos** (artifacts are kept for 14 days after a run).
5. Open the downloaded `jarvis-macos.zip` file and extract it to a folder on your Mac. Inside you will find the `JARVIS_*.dmg` disk image file and a `SHA256SUMS.txt` file listing checksums.

## 2. Install the app and pass Gatekeeper (Open Anyway)

Because JARVIS is an early alpha, the build is ad-hoc signed (`signingIdentity: "-"`) and is not notarised by Apple (decision D-13, spec §6.9). macOS Gatekeeper will block it from opening the first time.

Since macOS 15 Sequoia, Control-clicking or right-clicking no longer bypasses Gatekeeper (spec §6.9). Follow these steps:

1. Double-click the extracted `.dmg` file to open it (not checked on a Mac).
2. Drag the **JARVIS** app icon into your **Applications** folder (not checked on a Mac).
3. Eject the disk image in Finder (not checked on a Mac).
4. Open your **Applications** folder and double-click **JARVIS**.
5. macOS will display an alert stating that "JARVIS" cannot be opened because Apple cannot check it for malicious software (not checked on a Mac). Dismiss the dialog without moving the app to the Bin (click **Done** or **Cancel**, not checked on a Mac).
6. Open the Apple menu and choose **System Settings** (not checked on a Mac).
7. In the left sidebar, click **Privacy & Security** (not checked on a Mac).
8. Scroll down to the **Security** section (not checked on a Mac).
9. You will see a message that JARVIS was blocked, with an **Open Anyway** button (not checked on a Mac).
10. Click **Open Anyway** next to that message (not checked on a Mac).
11. When prompted, enter your Mac user password or use Touch ID, then confirm (click **Open** or **Open Anyway** on the final confirmation prompt, not checked on a Mac).

You only need to complete the **Open Anyway** steps the first time you run a newly downloaded build. Next, follow [`docs/first-run.md`](first-run.md) to add your API key and try your first commands.

## 3. Permissions macOS asks for and why

macOS requires apps to request user permissions before accessing sensitive system features.

### Microphone (`NSMicrophoneUsageDescription`)

macOS requires every audio capture app to declare a microphone description (`NSMicrophoneUsageDescription`), otherwise audio capture silently returns zeros (spec §6.7).

In `src-tauri/Info.plist`, JARVIS declares:

> _"JARVIS listens only while you use push to talk, and keeps the sound in memory only; it is never saved."_

- **The prompt**: the first time you press the **Hold or tap to talk** button in the installed app, macOS asks for microphone access displaying this description (not checked on a Mac).
- Click **OK** (or Allow) to allow JARVIS to capture your voice while holding the button (not checked on a Mac).
- **Privacy**: audio is kept in memory only while listening and is never written to disk (see [`docs/privacy.md`](privacy.md)).
- If you ever need to check or grant microphone permission later, go to **System Settings** → **Privacy & Security** → **Microphone** and ensure the toggle for **JARVIS** is turned on (not checked on a Mac).

### Automation for Spotify and Music (`NSAppleEventsUsageDescription`)

When you type quick music commands (`pause`, `play`, `next song`, `previous song`, `volume up`, `volume down`, `mute the music`, or `what's playing`), JARVIS controls either the Spotify app or the Music app using fixed AppleScripts (decision D-22, `docs/known-limitations.md`).

In `src-tauri/Info.plist`, JARVIS declares:

> _"JARVIS asks Spotify and Music to pause, skip and say what is playing, only when you tell it to."_

- **The prompt**: the first time you enter a music control command, macOS displays a permission prompt asking whether to allow JARVIS to control "Spotify" or "Music" (not checked on a Mac).
- Click **OK** (or Allow) to permit JARVIS to send playback and volume commands (not checked on a Mac).
- **If permission was denied**: if you clicked "Don't Allow", the music command will report that permission is needed. To grant permission, open **System Settings** → **Privacy & Security** → **Automation**, find **JARVIS**, and turn on the checkmark for **Spotify** or **Music** (not checked on a Mac).
- **Player rules on macOS** (`docs/known-limitations.md`):
  - Controls reach the Spotify and Music apps only (not web browsers or other players).
  - The music controls never start a player. Asking for a song by name (`play <song>`), a playlist, or liked songs starts Spotify if it is closed (decision D-25).
  - Playing music by name (`play <words>`) needs a one-time Spotify setup under **Settings** → **Music**; see [`docs/known-limitations.md`](known-limitations.md).
  - If both Spotify and Music are open, whichever player is currently playing is used (Spotify first on a tie).
  - Volume moves by approximately 10% per command, and `mute the music` toggles system sound mute.

## 4. What an ad-hoc rebuild does to permissions

JARVIS builds are ad-hoc signed (`signingIdentity: "-"` in `src-tauri/tauri.conf.json`).

Because an ad-hoc signature changes every time an application binary is rebuilt (spec §6.9):

- macOS identifies apps by their cryptographic code signature.
- When you install a new build or rebuild from source, macOS may treat the updated binary as a new app and prompt for Automation or Microphone permissions again (spec §6.9).
- For developers rebuilding frequently, spec §6.9 notes an optional workaround: create a self-signed code-signing certificate in Keychain Access and specify its name as `signingIdentity` to provide a stable local identity across builds.

## 5. Run from source (for developers)

Developers working directly in the repository can run JARVIS with live reload from source.

### Prerequisites

You need Git, Node.js (v22), pnpm (v10), a Rust stable toolchain, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for macOS (`README.md`).

### Running in development mode

From the repository root folder, run:

```bash
pnpm install
pnpm tauri dev
```

**Permission note for development builds**: When running `pnpm tauri dev`, macOS attaches permissions (such as microphone access or Automation events) to your terminal application (e.g. Terminal or iTerm2) rather than to the temporary JARVIS development process (spec §6.7, spec §6.9).

### Terminal shortcuts: `jarvis` and `jarvis-mock`

You can install two terminal shortcuts using `scripts/install-shortcuts.sh` (`README.md`):

```bash
scripts/install-shortcuts.sh
```

This creates symbolic links in `~/.local/bin` (`jarvis` and `jarvis-mock` pointing to `scripts/jarvis`) and adds `~/.local/bin` to your `PATH` in your shell configuration file (`~/.zshrc` on zsh, or `~/.bash_profile` on bash).

Open a **new** terminal window and run:

| Command       | What it starts                                                            |
| ------------- | ------------------------------------------------------------------------- |
| `jarvis`      | The real app (`pnpm tauri dev`, needs your Anthropic API key in Settings) |
| `jarvis-mock` | The mock backend: fake replies, scenario buttons, and a MOCK badge        |

Before starting, each script updates the repository (`git pull --quiet --ff-only`), but only when on branch `main` with no uncommitted changes. Set `JARVIS_NO_UPDATE=1` to skip the update.

To remove the shortcut links later:

```bash
scripts/install-shortcuts.sh --uninstall
```
