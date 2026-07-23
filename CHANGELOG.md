# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) once a
first release ships.

## [Unreleased]

### Added

- Preview rejects an invalid tag value instead of writing it (#82). A change
  plan now validates each proposed value and flags a rejected one; the preview
  marks that cell as an error (the state styled in #80/#76) while apply skips it,
  so the field keeps its current on-disk value. The first validated field is the
  year: a non-numeric or implausible year (e.g. `19x6`) is rejected, while a
  plain year or a dated `1996-05-01` passes. The flag rides on the plan DTO
  (`#[serde(default)]`, so older plans still deserialize) and is set at every
  plan source — tag edits, transforms, and Discogs import.

- Preview shown as a table-diff (#80). The staged change plan is no longer a
  vertical `Current → New` list but a table that mirrors the main file table, so
  a batch is scanned in the same layout the user reads it in. The core columns
  (File · Artist · Title · Album · Year) always show; one extra column is added
  per changed non-main field (Album Artist, Track, Genre, …) with an
  accent-underlined header, and a Cover column appears when a cover changes.
  Cells show the new value; unchanged cells are dimmed so changes pop, a folder
  move adds the new path line on the File cell (#37), and the File column stays
  pinned on horizontal scroll. The old value is on hover, or revealed
  struck-through under every changed cell via a "Show old values" toggle. Error
  (rejected value) styling is wired but latent until the backend flags a change
  invalid. (The design's per-row "include in this apply" checkbox is deferred
  until the apply path supports a partial plan.)

- Keyboard row navigation in the file table (#76). A roving focus moves between
  rows with ↑/↓ (drawing the new focus ring), and Space toggles the focused row's
  selection — so a keyboard-heavy tool's main surface is now operable without the
  mouse, complementing the existing click / ⌘ / Shift selection.

- Rich release picker in TAGGER (#27, step 2 of 2). Discogs search results are
  now expandable cards instead of a flat list: each shows a cover thumbnail
  (fetched in the background), the catalogue number as an accent chip, and
  country · year · format. Clicking a card lazily fetches the release and reveals
  its tracklist inline (checkbox · number · title · artist · duration) with a
  live selected count; Enable/Disable all, Auto-match, Embed cover and Import are
  all per-card. A List/Grid toggle switches to compact cover tiles. Backend: the
  Discogs search DTO now carries `thumb_url`, `country`, `label`, `format` and
  `catalog_number`. Nothing is written until Import goes through the usual
  preview/apply/undo path. (The match-confidence bar from the design is deferred
  until candidates are scored against the selection.)

- Native folder picker (#74). A "Browse…" button next to the library path opens
  the OS folder chooser and loads the picked folder (the scanner already
  recurses into its subfolders), so opening a library no longer means pasting a
  path. Built on `tauri-plugin-dialog`; outside the desktop shell it falls back
  to focusing the path field.

- Text transformations (#34): a "Transform…" dialog runs an ordered chain of
  cleanup rules over the selected files' tags or filenames, previewed and applied
  through the normal journaled path. Rules are find-and-replace (literal or
  regex, with whole-word and case-sensitivity switches), change case (lower,
  UPPER, Title, Sentence — with a data-driven exception list that keeps acronyms
  and roman numerals like `DJ` and `III` from being mangled), and remove
  diacritics (`Björk` → `Bjork`, expanding ligatures and `ß`). Rules can be
  reordered and scoped to all tags, one field, or the file name. A malformed
  rule (bad regex, unknown kind) is reported rather than silently doing nothing.

- Conditional sections in masks (#68). `[...]` renders only when a placeholder
  inside it resolved to something and is dropped whole otherwise, so one mask
  serves a library where some albums have a year and some don't:
  `[%albumartist%] - %album%[ (%year%)]/%disc%%track% [%artist% - ]%title%`.
  Sections nest, a missing tag inside one merely suppresses it (outside one it
  is still an error), and `'x'` quotes a literal `%`, `[` or `]`. In the extract
  direction a section becomes an optional group.
- `%catalognumber%` joins the addressable fields, mapped to the catalogue-number
  tag — it appears in real rename patterns and is Discogs' most precise key.

- Track numbers zero-pad to two digits when rendered from a mask (#65), so a
  plain alphabetical sort stays correct and a concatenated `%disc%%track%` reads
  as `101` (disc 1, track 01) rather than `11`, which a player would take for
  track eleven. Any placeholder can set its own width — `%disc:2%`, or
  `%track:1%` to opt out. Values that aren't purely numeric (`A1`, `1/12`) are
  left alone.
- Reorganize files into folders from a template (#37): a "Reorganize…" action
  renders a full relative path from a mask (`%albumartist%/%year% - %album%/
  %track% - %title%`), previews the moves, and applies them through the same
  journaled pipeline as a rename. Missing folders are created, and undo removes
  exactly the folders the batch created — a directory that already existed is
  never deleted, even if the rollback leaves it empty. Only literal slashes in
  the pattern create folders; tag values still have their separators stripped,
  and a pattern that would produce an empty component or climb out of the
  library is refused.
- Extended tag-field editor (#35): a "Fields…" dialog edits every field the
  model knows for the whole selection — album artist, track/disc numbers and
  totals, genre, comment, plus new first-class Composer, Publisher, BPM, ISRC
  and Key fields — and can add arbitrary custom fields. A field whose value
  differs across the selection shows `<multiple values>` and is left untouched
  unless typed into, so editing one field can't silently flatten the rest.
  Changes land in the same pending-edits buffer as inline table edits, and the
  new fields are usable as rename-mask placeholders (%composer%, %bpm%, …).
- Support every container the tag backend handles (#36): AAC, AIFF, WAV, Opus,
  Speex, Musepack, Monkey's Audio and WavPack alongside the original MP3/FLAC/
  OGG/M4A. These files were previously skipped by the scanner even though the
  preview player already decoded them. AIFF, WAV and AAC store their tags in
  ID3v2, so they now take the same concrete-tag write path as MP3 — otherwise
  adding them would have risked exactly the frame loss fixed in #52, and AIFF/WAV
  are where DJ software keeps cue points.
- Use track lengths as a matching signal (#64). Release track durations are now
  parsed and shown in the release view, and folded into the match score:
  agreement confirms, a large gap lowers confidence and is reported as a delta,
  but length never rejects a candidate on its own — provider durations are
  hand-transcribed and disagree with real files too often to be trusted that
  far. Adds order-preserving alignment by duration sequence for the case that
  matters most: a folder of `track01.mp3`-style files with no usable titles,
  where the ordered vector of lengths identifies the release on its own.
- Match provider candidates by content instead of result order (#53). A new
  matching module normalizes titles progressively (case, throwaway attributes
  like "Original Mix", punctuation, leading articles), takes an exact hit at any
  level, and otherwise falls back to a normalized Levenshtein similarity gated
  by a strictness threshold, with optional artist and duration checks. Remix
  credits are never stripped — a remix is a different recording. Discogs search
  results are now ranked by real similarity to the query rather than the order
  the API returned them, and an "Auto-match" action in the release view reorders
  the selected files to line up with the tracklist by title, so an import can no
  longer tag a whole album one title out of step.
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

### Changed

- Reorganized the TAGGER panel into ONLINE / EDITOR sub-tabs (#77). Online Discogs
  search and the release cards live under **ONLINE** with a pinned search header —
  only the results scroll now, not the whole panel; hand-editing tag fields and
  cover art live under **EDITOR**. The Discogs token moved out of the way behind a
  gear toggle (a proper Settings area is still to come) and is remembered as
  before.

- Typography scale and tabular numerals (#76). The ad-hoc font sizes, weights and
  letter-spacings across the UI now reference the design-system type tokens
  (`--text-*`, `--fw-*`, `--ls-*`), tidying the scale without changing how it
  looks. Figures that stack or are compared column-to-column — years, durations,
  track/disc counts, the selection count, player time — now use `tabular-nums`
  so their digits line up. (A bundled disambiguating mono is left as a token slot
  for later; the system mono ships today.)

- Table-row state layering (#76). A row can be hover + selected + dirty (per
  cell) + playing + keyboard-focused at once. Backgrounds are now ranked (dirty
  cell → selected row → hover) and the rest move to orthogonal channels that
  never fight the fill: the **playing** track is a left-edge accent bar (it was a
  full-row tint that overwrote the selection tint), and keyboard focus is an
  inset ring. Also adds latent per-cell error styling for a future rejected-value
  state.

- One "inert / unavailable" visual language (#76). Disabled controls, empty
  states and in-flight loaders now share a single diagonal-stripe motif (the
  release-cover placeholder, generalized): disabled buttons/fields get soft
  stripes under a muted label instead of a flat `opacity` fade, empty states are
  dashed striped panels, and a release's tracklist shows a shimmering skeleton
  while it fetches.

- Visible keyboard focus rings, and the design-system token layer they sit on
  (#76). Every control now shows a two-layer accent focus ring on keyboard
  navigation (`:focus-visible`, so plain mouse clicks stay quiet) — the app is
  keyboard-heavy but previously showed no focus at all. This also lands the
  foundation tokens from the Claude Design pass (type scale, line-heights,
  weights, spacing scale, radii, the focus-ring, the inert-stripe motif, and
  selection/dirty/error tints) that the rest of the states/inert/typography
  integration will build on.

- Split the accent into fill vs ink for text contrast (#76). A new `--accent-ink`
  token carries the accent where it is used as *text* (active tabs, brand mark,
  sort indicator, rule numbers), separate from `--accent` used as a *fill* behind
  white text (buttons, selection, tab underline). This lets the accent-as-text
  clear small-text contrast independently of the fill — the win is in dark mode,
  where the fill green as small text was borderline.

- Reworked the main layout around mode tabs and a persistent file table (#27,
  step 1 of 2). The pile of toolbar buttons that each opened a modal is gone;
  instead the file table is the permanent subject and four mode tabs —
  **RENAMER** (rename mask + reorganize), **TAGGER** (Discogs online + field
  editor + cover), **GENERATOR** (transform/cleanup), **EXPORTER** — swap only a
  right-hand panel. The panel collapses and its divider drags, so the table can
  take the full width. A Files/Preview tab over the table shows every mode's
  change plan in one place (Apply/Discard), and a status line tracks the
  selection count. Selection is now a first-class set that survives re-renders
  (sort, reorder, auto-match, staging edits) instead of living in the DOM and
  being silently reset to "all": click selects a row, ⌘/Ctrl toggles, Shift
  ranges, and double-clicking a group's name toggles that whole group (its caret
  collapses it). Cells edit on double-click; the TAGGER field grid follows the
  current selection. The accent is now the brand green with a dedicated red kept
  for danger (errors, deletions, the "old" side of a diff); the table is a
  compact monospace. Step 2 (a richer release picker with cover thumbnails) is
  tracked on #27. Design follow-ups (colour shades, resizable columns) are on
  #76.

### Fixed

- Undo is scoped to the currently open library (#75). The undo journal is shared
  across every library you open, so after working in one library and opening
  another, the second library's Undo offered the first's batches — and undoing
  one then failed with "path resolves outside the allowed root", stranding it.
  History now lists only batches whose files sit under the open library (matched
  against both the raw and canonicalized root), so Undo always applies to what
  you are actually looking at.

- Discogs disambiguation suffixes are stripped from every artist in a credit,
  not only the last one (#69). Discogs tags each artist individually, so a
  joined credit carries them mid-string — `Zolex (2), Carat Trax (3)`,
  `Oxygen (9) feat. Nbg (2)` — and only the trailing one was removed. Search
  results were worse off: they arrive as one combined "Artist - Title" string
  and were not cleaned at all, so every suffix survived into the candidate list.
  A suffix is now removed wherever it is followed by a name boundary, which
  keeps genuine parentheticals like `Godspeed You! Black Emperor (F#A#)` and
  `Apollo (440) Sound` intact.

- Folder masks respect the platform separator (#71). Only `/` counted as a
  directory boundary, so a pattern written with `\` — the natural form on
  Windows, and what an imported configuration carries — was not recognised as
  having folders at all: the `..` and empty-component guards never saw the
  components, and on macOS the backslash ended up as a literal character inside
  one long file name. Silently doing the wrong thing rather than failing. Both
  separators are now accepted in a pattern, and the path is built component by
  component so the platform supplies its own separator. Tag values keep having
  both stripped, so a value still cannot inject a directory.

- Masks accept two placeholders in a row (#65). `%disc%%track%` was rejected as
  ambiguous at parse time, which also blocked rendering — but only *extraction*
  is ambiguous there, since nothing says where one value ends and the next
  begins. The check moved to `extract`, so such a pattern now renders fine and
  only refuses the filename-to-tags direction.

- Tag writes no longer destroy frames the tag model can't express (#52).
  `TagMap` is text-only, so rebuilding a tag from it wiped everything else on
  every edit, import or rename — DJ cue points and loops, ratings, ReplayGain
  and other private/binary frames. MP3 is now written through its concrete
  ID3v2 tag, because lofty's generic tag doesn't even surface those frames when
  reading, so an MP3 round-tripped through it lost them silently; non-text
  frames are carried over while text frames come from the model, so clearing a
  field still clears it. Cover embed/remove take the same path. Other formats
  keep the generic tag but now start from the file's existing one, not a blank.
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
