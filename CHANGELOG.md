# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) once a
first release ships.

## [Unreleased]

### Added

- Exporters (#19): an "Export…" action writes the selected tracks into the
  opened library as an extended M3U playlist (relative entry paths and real
  track lengths), a CSV of the tag columns (RFC 4180 quoting), or a text report
  rendered from a mask template using the same placeholders as rename masks.
  Read-only — the audio files are never modified. Export file names must be bare
  names, so an export can't be steered outside the library.
- Expand All / Collapse All for groups (#32): buttons next to the Group selector
  (shown only while grouped) toggle every group at once, reusing the in-place
  collapse path so selection and in-progress edits survive.
- Group the track table (#20): a "Group" selector groups rows by folder, artist,
  or album under collapsible headers showing each group's track count; clicking a
  header collapses/expands it without a re-render, so selection and in-progress
  edits survive. Grouping is strictly a view concern — it never reorders the
  underlying track list, and position-based mapping (rename masks, Discogs
  import) now explicitly follows that list rather than the visual row order.
  Grouping by release id is listed but disabled until a release identifier is
  stored on tracks.
- Gapless playback (#30): the preview player now runs on a native rodio/
  Symphonia backend instead of a WebView `<audio>` element. A dedicated audio
  thread keeps the current and next track queued in one sink, so tracks play
  back-to-back with no gap — seamless on continuous/mixed compilations. As a
  bonus it decodes every format we handle, including OGG (which the old WebView
  player couldn't play). The UI drives it via `player_*` commands and polls a
  status snapshot for the seek bar / time; auto-advance (#29) is now realized by
  the backend queue. CI installs `libasound2-dev` (ALSA) for the Linux build.
- Player auto-advance (#29): when a track finishes, the player automatically
  plays the next visible track (respecting the current sort/filter/manual
  order), continuing down the list until it ends or the user stops. An
  unplayable file (e.g. an unsupported format) is skipped mid-run rather than
  halting playback; a manually chosen unplayable file just reports and stops.
- Always-visible player controls (#31): the player bar stays docked once a
  library is open, showing a disabled idle state ("No track loaded", `0:00 /
  0:00`) instead of appearing only during playback and vanishing on stop.
- Built-in preview player (#28): a ▶ button on each track row auditions the
  file in an in-app player bar (play/pause, stop, seek, elapsed/total time) — no
  external player, no leaving the app. The backend streams the file's bytes to
  the webview as a Blob, which plays the formats WKWebView supports (MP3, M4A,
  FLAC); unsupported files (e.g. OGG) surface a friendly message instead of
  failing silently. Preview-only: reads bytes, never touches the tag pipeline.
- Genre tag from Discogs Style, not Genre (#26): a Discogs import now fills the
  genre tag from the release's `styles` (e.g. `Trance/Tribal/Techno`) rather
  than the coarse `genres` (e.g. `Electronic`), which is closer to what a genre
  tag usually means. Multiple styles are joined with `/` (matching the common
  library convention); releases with no styles fall back to their genres. The
  provider now exposes `genres` and `styles` separately instead of merging them.
- Export embedded cover art to disk (#25): an "Export cover" toolbar action
  saves the embedded front cover of the selected files as a `cover.<ext>`
  sidecar next to each one (extension from the cover's MIME type). Read-only for
  the audio files — it never touches the tag-write/undo path. Tracks sharing a
  folder collapse to a single `cover.jpg` (one write, not one per track), and
  files without an embedded cover are reported as skipped rather than failing.
- Fetch cover art from Discogs (#24): the release detail view now shows the
  release's primary image (downloaded through the backend, since Discogs image
  URLs need the token + User-Agent the webview can't send) with an "Embed
  cover" action that embeds it into the selected files. The fetched bytes reuse
  the same preview/apply/undo cover path as a locally chosen image.
- Cover art embed (#18, core): embed a front cover from a local image file
  into the selected tracks, previewed with a thumbnail and applied through the
  same journaled/undoable path as tags (a new cover change kind in the plan,
  executor, and SQLite journal — undo restores the previous cover). Fetching
  covers from Discogs (#24) and exporting them (#25) are tracked separately.

### Fixed

- Tag writes no longer strip embedded artwork: `TagEngine::write` rebuilt the
  tag from the text fields only, so any edit/import/rename silently dropped the
  cover. It now carries existing pictures over.

- Sort the track table by column (#21): click a header (File/Artist/Title/
  Album/Year) to sort, click again to reverse; an arrow marks the active
  column. Sorting reorders the underlying list so position-based mapping
  (rename masks, Discogs import) follows the visible order; a manual
  drag-reorder supersedes the column sort.
- Filter the track table (#22): a search box hides rows that don't match a
  substring across the filename and tag columns; the count shows shown/total.
  Filtering is view-only — selection and mapping operate on the visible rows.

- Unified pending-edits model (#23): inline cell edits and Discogs import now
  feed one buffer, so they compose into a single preview and Apply instead of
  two disconnected flows. Import merges into pending edits without overwriting
  a field the user already edited by hand (manual wins), edited/imported
  values both show as dirty cells, and pending tag edits survive a rename
  (remapped to the new paths) rather than being silently lost.

- Discogs release import in the GUI (#10): a Discogs panel (token + search →
  candidate list → release tracklist) imports metadata onto the selected
  files, previewed and applied through the same journaled/undoable path.
  Following the TagScanner model, the user resolves the mapping explicitly:
  each release track has a checkbox (with Enable/Disable all), and files can
  be drag-reordered in the main table so they line up. Enabled tracks map onto
  the selected files in order; the track number comes from the release track's
  own position (so an aligned file keeps its real number), and files with no
  matching track get only album-level fields. `App::preview_import` builds the
  plan from the user's resolved selection.

### Fixed

- Discogs import no longer scrambles tags: the previous version mapped release
  tracks onto files by scan order (unrelated to the tracklist), silently
  writing wrong artist/title/track to a partial selection. Import is now
  user-resolved (see above), and the track number is never invented from the
  selection index.
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
