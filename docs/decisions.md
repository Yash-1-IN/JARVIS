# Decision log

One line per decision. Newest at the bottom. IDs match `docs/spec.md` §15.
Status: proposed / decided / superseded.

| ID | Date | Decision | Status |
| --- | --- | --- | --- |
| D-01 | 2026-11-01 | Distribution intent: open source, project licence `MIT OR Apache-2.0`. Dependency licence allow-list: MIT, Apache-2.0 (incl. `WITH LLVM-exception`), BSD-2-Clause, BSD-3-Clause, 0BSD, ISC, Zlib, Unicode-3.0, MPL-2.0 (used unmodified), CDLA-Permissive-2.0 (data, e.g. `webpki-root-certs`; added 2026-11-01 for #7). Anything else needs a new decision. Check with `pnpm licenses list --prod` and `cargo metadata` (or `cargo deny`) before adding a dependency. | decided |
| D-13 | 2026-10-31 | No Apple Developer membership: macOS builds are ad-hoc signed (`signingIdentity: "-"`), not notarised; Windows installers unsigned for alpha. Revisit before any public release. | decided |
