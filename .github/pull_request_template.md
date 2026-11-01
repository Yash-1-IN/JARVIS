<!-- Place this file at .github/pull_request_template.md -->

## Task
Closes #<!-- issue number --> · Owner: Developer A / Developer B

## What changed
<!-- One line per file: path — why. -->

## Checks run
<!-- Paste the result line of each command from CLAUDE.md. -->
- [ ] `cargo fmt --check`, `cargo clippy`, `cargo test`
- [ ] `pnpm typecheck`, `pnpm lint`, `pnpm test`, `pnpm format:check`
- [ ] Generated types unchanged, or regenerated because this PR changes contract types

## Boundaries
- [ ] Touches only my own modules
- [ ] Touches the other developer's modules or a shared contract: explain why below

## Status
- Implemented:
- Mocked:
- Not done:

## Manual test on real hardware
<!-- Needed for anything involving the microphone, speakers, the running app, macOS/Windows, or performance. -->
<!-- List the exact steps. The developer who runs them pastes results as a PR comment before merge. -->
- [ ] Not needed
- [ ] Needed, steps:

## Decisions
- [ ] No new decisions
- [ ] Added to `docs/decisions.md`:

## Risks and limitations
