# Decision log

One line per decision. Newest at the bottom. IDs match `docs/spec.md` §15.
Status: proposed / decided / superseded.

| ID | Date | Decision | Status |
| --- | --- | --- | --- |
| D-01 | 2026-11-01 | Distribution intent: open source, project licence `MIT OR Apache-2.0`. Dependency licence allow-list: MIT, Apache-2.0 (incl. `WITH LLVM-exception`), BSD-2-Clause, BSD-3-Clause, 0BSD, ISC, Zlib, Unicode-3.0, MPL-2.0 (used unmodified), CDLA-Permissive-2.0 (data, e.g. `webpki-root-certs`; added 2026-11-01 for #7). Anything else needs a new decision. Check with `pnpm licenses list --prod` and `cargo metadata` (or `cargo deny`) before adding a dependency. | decided |
| D-13 | 2026-10-31 | No Apple Developer membership: macOS builds are ad-hoc signed (`signingIdentity: "-"`), not notarised; Windows installers unsigned for alpha. Revisit before any public release. | decided |
| D-15 | 2026-11-01 | Usage and spend caps reset on UTC day and month boundaries for alpha (no timezone dependency). The UI shows the next reset in local time. Local-midnight resets can come later. | decided |
| D-16 | 2026-11-01 | Model prices live in `src-tauri/resources/pricing.json` keyed by model id, not user-editable. `settings_get` returns the priced ids as `availableModels`; an unknown model is rejected rather than guessed. | decided |
| D-17 | 2026-11-01 | Tool loop: at most 5 tool calls per turn; a 6th in the same model reply gets an error result and the turn ends with a plain limit message. A medium/high call waits for the approval card up to 65 s (gateway expiry is 60 s). A failed or cancelled turn removes everything it added to the history (user message, tool calls, results). Tool results reach the model labelled as untrusted data and truncated to 16,000 characters. An unregistered tool name, or arguments outside the allowed folders and apps, are refused and logged as `prohibited` / `not_run` with the cleaned tool name only (no targets, nothing the model supplied beyond the name); malformed arguments (`Invalid`) are not logged. | proposed |
| D-10 | 2026-11-01 | Database encryption at rest (spec §15): do not adopt SQLCipher in alpha; rely on OS disk encryption and say so in the privacy doc. The database holds conversation history and the memories the user confirmed; it never holds API keys, audio or file contents. Revisit if D-01's audience includes machines without disk encryption. | **proposed** (needs the owner) |
| D-11 | 2026-11-01 | History retention default (spec §8.3, §15): 30 days, user-selectable off / 7 / 30 / 365 days / forever, with a "delete all history" button. | **proposed** (needs the owner) |
