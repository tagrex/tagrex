# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) once a
first release ships.

## [Unreleased]

### Added

- Discogs release import in the GUI (#10): a Discogs panel (token + search →
  candidate list) maps a chosen release's tracklist onto the selected tracks
  and previews the resulting tag changes, applied through the same journaled/
  undoable path. `App::preview_release_import` fetches the release and maps it
  by table order — album/albumartist/year/genre to every file, title/artist/
  track number to files that line up with a release track — dropping no-op
  edits; the position mapping is a pure function unit-tested offline.
- Inline tag editing in the GUI (#9): artist/title/album/year cells in the
  track table are editable; edited cells are highlighted, "Preview edits"
  shows a field-level current→new diff, and Apply writes through the same
  transactional path as renames (so tag edits are journaled and undoable).
  Backed by `App::preview_tag_edits`, which reads each file's current value as
  the change's `old` and drops no-op edits.

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
- Transaction pipeline (#4): `Executor::apply`/`undo` — the only writers in
  the codebase. `apply` takes an `allowed_root` and rejects the whole plan
  (before writing anything) if any path resolves outside it, if the on-disk
  state is stale relative to the plan, or if the plan carries a rename.
  Applied batches are recorded so `undo` can restore each field's previous
  value. `VecJournal`, an in-memory `UndoJournal`, backs the pipeline until
  the persistent SQLite journal (#5) lands. Tag writing and renaming are
  separate operations (like TagScanner's separate tabs); this increment does
  tag writes only, rename tracked separately.
- Persistent SQLite journal (#5): `SqliteJournal` (via `rusqlite`, bundled
  SQLite) durably records batches across three normalized tables so an
  applied batch survives an application restart — the "renamed 8,000 files,
  closed the app, realized the mask was wrong" scenario is now recoverable
  after reopening. Batch ids are assigned by the journal (database
  autoincrement) rather than a process-local counter, so they stay unique
  across restarts.

- Rename execution in `Executor` (#8): a plan's `rename_to` moves are now
  applied (and reversed on undo). Within a file, tags are written before the
  rename so a mid-move failure leaves the file at its old path with new tags;
  undo reverses the move first, then restores tags. The whole plan is
  pre-flighted for rename safety — targets must resolve inside `allowed_root`,
  must not already exist on disk, and two files may not target the same path
  (`PlanError::RenameCollision`). Chained/cyclic renames (a target that is
  another file's source) are conservatively rejected for now.

- Discogs metadata provider (#6): `DiscogsProvider::search`/`fetch_release`
  over a blocking `ureq` client (personal-token auth, required User-Agent).
  429 responses surface as `ProviderError::RateLimited` with the `Retry-After`
  value; auth/not-found/other statuses are mapped too. Discogs' numeric artist
  disambiguation (`Artist (3)`) is stripped through a core transform-pipeline
  step (`StripDiscogsSuffix`). Response mapping is factored into pure functions
  and unit-tested against fixture JSON (no network); a `discogs_search` example
  exercises the live API with a token.

- Application command layer (#7): the `tagrex` crate is a library exposing
  `App` — the thin, GUI-agnostic surface the shell forwards intent to (open
  library, list tracks, preview a mask rename, apply, undo, history, Discogs
  search/fetch). Data crosses the boundary as serde DTOs so `tagrex-core`
  stays serialization-free. The library root doubles as the executor's
  `allowed_root`.
- Tauri 2 desktop shell (#7): the `tagrex` binary is now a Tauri app — a thin
  window whose `#[tauri::command]`s are one-line forwards into `App`, over a
  static HTML/CSS/JS frontend (no npm/JS-framework build step) that renders
  the track table and the current→new rename preview. Verified end to end on
  the real native window: open a folder → preview by mask → apply (real
  renames + a persisted batch in the SQLite journal) → undo (reverted on disk,
  batch cleared). Only the GUI crate needs a modern toolchain (Tauri 2 raises
  the MSRV to 1.82); the core crates stay at 1.75.

### Changed

- `UndoJournal::record` now returns the journal-assigned `BatchId` instead of
  `()`; the journal owns id assignment so ids survive restarts. `TagField`
  gains a lossless `to_storage_key`/`from_storage_key` codec for persistence.

### Fixed

- `TagEngine::read` now also recognizes `RecordingDate` (ID3v2.4 `TDRC`) as
  `TagField::Year`, not just the legacy `Year` (`TYER`). Verified against
  TagScanner-tagged files, which write the year exclusively through
  `RecordingDate` — without this, `Year` was silently empty for most
  real-world files.
