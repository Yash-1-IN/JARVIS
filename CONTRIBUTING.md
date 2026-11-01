# Contributing

Read [`CLAUDE.md`](CLAUDE.md) first; it holds the rules the whole project follows, for people and for
coding assistants alike.

## Short version

- `main` is the only integration branch (decided in issue #12). One task, one branch, one pull request into `main`. The cross-review rules between the two developers are in issue #15.
- Fill in the pull request template. Say what you ran and what you could not run (for example,
  anything that needs a microphone, a speaker, macOS or Windows).
- Changes to the shared contract (`src-tauri/src/ipc/`, `src/types/generated/`, `docs/contracts.md`) go
  in their own pull request first. Never edit the generated types by hand; run `pnpm gen:types`.
- Record a decision in [`docs/decisions.md`](docs/decisions.md) in the same pull request.
- A pull request needs one approval from someone other than its author, and green CI on all three
  operating systems.

## Dependencies and licences

Add a dependency only with a one-line reason in the pull request, and check its licence against the
allow-list in decision D-01. No GPL or non-commercial components.

## Security-sensitive changes

Anything that touches `permissions/`, `tools/`, paths, API keys or what the model can reach needs a
test that shows the unsafe case is refused. See [`docs/threat-model.md`](docs/threat-model.md).
