# Licence audit (D-01)

Checked 2026-11-01 against the allow-list in `docs/decisions.md` (D-01). The project itself is
`MIT OR Apache-2.0`.

## Rust dependencies (shipped in the app)
Source: `cargo metadata --all-features` for `src-tauri` (all target platforms).

- **564** third-party crates.
- **0** outside the allow-list. Every crate offers at least one allowed licence (an `OR` expression passes
  if any option is allowed; an `AND` needs all of them).
- Most common: `MIT OR Apache-2.0` (about 330 with its spelling variants), `MIT` (133), `Unicode-3.0` (18),
  `Zlib OR Apache-2.0 OR MIT` (18), `MPL-2.0` (5, used unmodified, allowed by D-01), `ISC` (4),
  `BSD-3-Clause` (3), `Unlicense OR MIT` (10, taken under MIT).
- **SQLite itself** (bundled by `rusqlite`) is public domain. That is not on the original list; the owner
  approved SQLite and only SQLite on 2026-11-01 (D-01 amended in #36).

## Frontend dependencies (shipped in the app)
Source: `pnpm licenses list --prod`.

- **101** production packages on 2026-11-01 (after the 3D hologram, #48, added `three`, `@react-three/fiber` and a few small helpers): 98 MIT, 1 ISC, 1 BSD-3-Clause, 1 `Apache-2.0 OR MIT`. All allowed. The numbers move whenever a package is added; re-run the command below after such a change.

## Development-only packages (not shipped)
About two hundred further packages are used only to build and test (307 in total on 2026-11-01, before the hologram work). Besides the licences above they include
`MIT-0` (2), `MPL-2.0` (3), `BlueOak-1.0.0` (2), `CC0-1.0` (1) and `CC-BY-4.0` (1, browser-compatibility
data). None of them is included in the installers. If the project ever redistributes its build tooling,
these need their own decision.

## Not covered yet
- **Speech models and voices** (milestone M3): none exist in the app yet. Each model needs its licence
  checked before it is added (the spec flags Piper, espeak-ng and openWakeWord pretrained models).
- **Artwork and fonts**: the app uses Tauri's default icons and the system fonts. Replace the icons before a
  public release.
- **The name "JARVIS"** is a Marvel trademark. Fine for a private project; D-19 (proposed) records that a release name must be chosen before any public or paid release, and that the persona file's reference to the film character goes with it.

## How to repeat it
```bash
cd src-tauri && cargo metadata --format-version 1 --all-features > meta.json
pnpm licenses list --prod
```
then compare each licence expression with the D-01 list.
