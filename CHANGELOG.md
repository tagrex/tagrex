# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) once a
first release ships.

## [Unreleased]

### Added

- Workspace skeleton: `tagrex-core` (tag model, mask engine, transform
  pipeline, transaction pipeline, undo journal — all module signatures in
  place, `TagEngine` I/O still `todo!()`), `tagrex-providers-discogs`
  (provider trait implementation, HTTP client still `todo!()`), and a
  placeholder `app` binary proving the workspace links.
- CI on GitHub Actions: `cargo fmt --check`, `clippy -D warnings`, `build`,
  `test`.
- Tracking issues for the remaining implementation order from
  `docs/architecture.md` (#1-#7).
- `TagEngine::read`/`write` wired up to `lofty` (#1): the ten first-class
  `TagField`s map to `lofty` `ItemKey`s in both directions, `Custom` fields
  round-trip through `ItemKey::Unknown`.
- `read_tags` example (`cargo run -p tagrex-core --example read_tags --
  <path>`): read-only manual check of what `TagEngine` sees in a real file,
  no GUI required.
- Directory scanner (#2): `scan` walks a tree with `walkdir` and lazily
  yields supported audio files instead of collecting them up front, per the
  50k+ files requirement in `docs/architecture.md`. `scan` example for
  manually checking it against a real library.
- Mask engine (#3): `Mask::parse`/`render`/`extract`. Both directions are
  derived from the same parsed segment list — `render` substitutes it,
  `extract` compiles it into one anchored, escaped regex — so there's no
  second matcher to drift out of sync with `render`. Placeholders are
  limited to the ten first-class `TagField`s for now; `Custom` fields aren't
  addressable from a mask yet.

### Fixed

- `TagEngine::read` now also recognizes `RecordingDate` (ID3v2.4 `TDRC`) as
  `TagField::Year`, not just the legacy `Year` (`TYER`). Verified against
  TagScanner-tagged files, which write the year exclusively through
  `RecordingDate` — without this, `Year` was silently empty for most
  real-world files.
