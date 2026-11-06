# JARVIS

A voice-first desktop assistant for Windows and macOS (Linux builds for development).
Tauri 2 and Rust on the backend, React and TypeScript on the front.

> **Status: early alpha.** It is built milestone by milestone. This README says what works today;
> it does not describe the finished product.

## What works today

- Text chat with a cloud model (Anthropic), streamed to the window. You add your own API key; it is
  checked once and stored in the operating system's keychain, never in a file.
- Spend meter with daily and monthly caps, checked before every request.
- Quick local commands (stop, mute, repeat) that never touch the cloud.
- Settings for the model, caps, allowed folders and allowed apps.
- An activity view that lists every tool call, including refused ones, newest first.
- Tools the model can ask for, all gated by a permission layer in Rust: `time.now`, `files.search`,
  `files.read_text`, `files.move`, `files.rename`, `apps.list_allowed`, `apps.launch`, `system.status`,
  `memory.propose` and the music tools (`music.control`, `music.now_playing`, `music.play`,
  `music.play_playlist`, `music.play_liked`, with Spotify). Anything that changes or reads your files needs
  your approval first, on an approval card; approvals are single-use and expire after 60 seconds.
- Push to talk: **Hold or tap to talk** turns your speech into text on this computer (Moonshine, after a
  one-time model download in Settings → Models) and runs it as a normal request.
- Conversation history and memories you confirm, kept on this computer.

## What does not work yet

- Voice: spoken replies (text-to-speech) are not built, and the wake word cannot be turned on from the window yet (it is offered on Windows and Linux only).
- Compact orb mode, tray icon and start at login.
- Signed installers. CI builds unsigned (Windows) and ad-hoc signed (macOS) installers as
  artifacts of the `installers` workflow; macOS needs System Settings → Privacy & Security → Open Anyway the first time.

## Run it from source

You need Git, Node.js, pnpm, a Rust toolchain and the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system.

```bash
pnpm install
pnpm tauri dev
```

The app opens with no key. Open settings, paste an Anthropic API key, and choose a model.

### Shortcuts: `jarvis` and `jarvis-mock`

Run this once from the repository folder.

macOS and Linux:

```bash
scripts/install-shortcuts.sh
```

Windows (PowerShell or Command Prompt, from the repository folder; use a normal window, not
"Run as administrator"):

```bat
.\scripts\install-shortcuts.cmd
```

Then open a new terminal window and, from any folder, type:

| Command       | Starts                                                            |
| ------------- | ----------------------------------------------------------------- |
| `jarvis`      | the real app (needs your API key in Settings)                     |
| `jarvis-mock` | the mock backend: fake replies, scenario buttons and a MOCK badge |

Before starting, each one updates the checkout, but only when that is safe: on `main` with no
local changes, fast-forward only. On another branch, or with local changes, it leaves things alone
and says so. Set `JARVIS_NO_UPDATE=1` to skip the update. To remove the commands, run
`scripts/install-shortcuts.sh --uninstall` (Windows: `.\scripts\install-shortcuts.cmd -Uninstall`). An old `alias jarvis=…` line in your shell startup file
would take priority over the command; the installer tells you if it finds one. On Windows the
installer writes two small `.cmd` files into `%USERPROFILE%\.local\bin` and adds that folder to your
(user) PATH; it needs no change to PowerShell's script policy and refuses to overwrite a file it did not
write. `scripts/test-shortcuts.sh` and `scripts/test-shortcuts.ps1` check the scripts without starting
the app.

## Check your changes

```bash
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd .. && pnpm typecheck && pnpm lint && pnpm test && pnpm format:check
pnpm gen:types && git diff --exit-code src/types/generated
```

## How it is built

| Read this                                      | For                                                             |
| ---------------------------------------------- | --------------------------------------------------------------- |
| [`docs/spec.md`](docs/spec.md)                 | The engineering specification                                   |
| [`docs/contracts.md`](docs/contracts.md)       | The commands and events between Rust and the UI                 |
| [`docs/decisions.md`](docs/decisions.md)       | Decisions and their status                                      |
| [`docs/threat-model.md`](docs/threat-model.md) | What the permission layer defends against, and what it does not |
| [`CONTRIBUTING.md`](CONTRIBUTING.md)           | How to work on it                                               |
| [`SECURITY.md`](SECURITY.md)                   | Reporting a vulnerability                                       |

The short version of the safety design: the model only proposes. A gateway in Rust decides, checks
every path against the folders you allowed, binds each approval to the exact arguments, and treats
anything a tool returns as untrusted.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
