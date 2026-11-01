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
- Tools the model can ask for, all gated by a permission layer in Rust: `time.now`, `files.search`,
  `files.read_text`, `files.move` and `files.rename`. Anything that changes or reads your files needs your
  approval first (the backend is done; the on-screen approval card is still being built); approvals are single-use and expire after 60 seconds.

## What does not work yet

- Voice: wake word, speech-to-text and text-to-speech are not built.
- Launching apps, system status and memory.
- Packaged installers. Run it from source for now.

## Run it from source

You need Git, Node.js, pnpm, a Rust toolchain and the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system.

```bash
pnpm install
pnpm tauri dev
```

The app opens with no key. Open settings, paste an Anthropic API key, and choose a model.

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
