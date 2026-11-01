# Decision log

One line per decision. Newest at the bottom. IDs match `docs/spec.md` §15.
Status: proposed / decided / superseded.

| ID | Date | Decision | Status |
| --- | --- | --- | --- |
| D-01 | | Distribution intent: personal / open source (which licence) / commercial. Licence allow-list: | proposed |
| D-13 | 2026-10-31 | No Apple Developer membership: macOS builds are ad-hoc signed (`signingIdentity: "-"`), not notarised; Windows installers unsigned for alpha. Revisit before any public release. | decided |
| D-15 | 2026-11-01 | Usage and spend caps reset on UTC day and month boundaries for alpha (no timezone dependency). The UI shows the next reset in local time. Local-midnight resets can come later. | decided |
| D-16 | 2026-11-01 | Model prices live in `src-tauri/resources/pricing.json` keyed by model id, not user-editable. `settings_get` returns the priced ids as `availableModels`; an unknown model is rejected rather than guessed. | decided |
