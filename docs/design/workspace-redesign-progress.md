# Workspace redesign — progress & handoff

> **Living checkpoint.** This file is the single source of truth for the
> multi-slice workspace redesign so any Claude instance (or the user) can resume
> mid-stream. Update it at every slice boundary. Committed to git so it travels
> with the code.

## The plan

Redesign driven by the brief **`docs/design/workspace-layout-iconography.md`**
and the Claude Design deliverable in the "TagRex Design System" project
(`projectId b0312bbb-86f3-4e6a-a88c-06c8d9dc6e10`), files
**`foundations/workspace.html`** + **`foundations/workspace.css`** — pull them
with the `DesignSync` tool (`get_file`). `workspace.css` is split into
**`A · TRANSFERABLE`** (production classes authored to lift into
`app/ui/style.css`, sections A0–A8) and **`B · MOCK SCAFFOLD`** (specimen board —
do NOT transfer). The HTML answers the open design questions **Q1–Q5** inline and
carries the icon sprite + mock frames.

New IA: **top tabs = operations; the left area is always the file table; the
table enters a diff-state in place when a plan is staged (Preview dissolves);
Duplicates becomes a top-level DEDUPLICATOR mode.** The old
`Files | Preview | Duplicates` view strip disappears entirely once slices 3+4 land.

Agreed order: **1 → 2 → 3 → 4**, each its own GitHub issue.

## Status

| Slice | Scope | Design §| Issue | State |
|------|-------|--------|-------|-------|
| 1 | Inline-SVG icon set (retire unicode/emoji glyphs) + native-control normalization (`<select>` caret, search-clear, scrollbars) | A0, A3 | [#115] | ✅ shipped `4f1e488` |
| 2 | Mode tabs icon + label, with a `compact-tabs` icon-only fallback | A1 | [#116] | ✅ shipped `bbf6aeb` |
| 3 | **In-table diff-state** — dissolve the Preview view; the table shows the staged diff in place + a floating Apply/Discard bar; per-row apply moves to the sel column | A4, A5, A6 | [#117] | ⏳ NEXT — filed + analysed, build plan below |
| 4 | **DEDUPLICATOR** mode — promote Duplicates to a 5th top tab; controls into the right panel; grouped read-only results in the main area; view strip fully removed | A7, A8 | _not filed_ | ⏳ pending |

Both shipped slices are on `main`, signed, pushed, and bundled/relaunched.
The icon sprite already in `app/ui/index.html` includes `i-dedup`, `i-lock`,
`i-corner`, `i-arrow-right` — everything slices 3+4 need is present.

## What each remaining slice must do

### Slice 3 — in-table diff-state (design A5/A6, answers Q1+Q2) — **issue [#117]**, analysed, ready to build

**Current system (as read 2026-07-31, `app/ui/app.js`):**
- Data model: `previewPlan` = `PlanDto { description, changes: [{ path, rename_to,
  tag_changes: [{field, old, new}], cover_change }] }`; `previewSource` is one of
  `rename | edits | transform | cover | clear` (7 call sites set them: `preview`
  ~1294/`preview_rename`, `previewEdits`/`preview_tag_edits`, transform ~2159,
  move ~2189, cover embed/remove ~1432/1454, clear ~1477, import via edits).
- `renderPreview(plan)` (~1088) → `showView("preview")` + `renderPreviewDiff`
  (~1235) builds a SEPARATE `.diff` mirror table in `#preview-diff`, with its own
  `.diff-sel` per-row checkboxes + `.diff-sel-all`, `updateApplyFromChecks`, and
  a `show-old` class toggled by `#show-old`.
- `apply()` (~1333) reads ticked paths from `.diff-sel:checked` in `#preview-diff`,
  filters `previewPlan.changes` to that subset, `invoke("apply_plan")`, then for
  `wasRename` calls `remapEditsAfterRename`, for `wasEdits` drops applied paths
  from `edits`. `discardPreview()` (~1067) clears plan (+ resetEdits for edits).
  `showView` (~991) toggles `#files-view`/`#preview-view`/`#duplicates-view`.
- `renderTracks`/`appendTrackRow` (~812) build the main `#tracks` rows; the sel
  column checkbox reflects the `selection` Set; cells are `td.editable`, dirtied
  from the `edits` buffer.

**Build plan:**
1. Build `diffByPath` = Map(path→change) from `previewPlan` when set. Make
   `appendTrackRow` diff-aware: if the row has a change → `tr.staged`, tint the
   changed visible cells `td.dirty` (`td.error` for invalid), render the File
   cell as `.fcell` with `.fname` (new name from `rename_to`) + `.fpath`
   (`i-corner` + new relative dir) and a `.cell-old` old name; put the per-row
   **apply tick** in the sel column (checked by default, tracked in a new
   `applySelection` Set — NOTE the sel column's meaning switches from `selection`
   to apply-scope while in diff-state). Rows with no change → `tr.untouched`.
   Changed fields outside `visibleColumns` still apply but won't show a cell
   (the design's mock only diffs visible columns) — acceptable; the count covers
   them.
2. Add a floating `.diff-actionbar` over the table pane when a plan is staged:
   count from ticked applies, plan name (`previewPlan.description`), a **Show old
   values** toggle (adds `show-old` to `#tracks`), Discard, Apply. Lift CSS A5+A6.
3. Rewire `apply()` to read ticked paths from the sel-column apply-ticks
   (`applySelection`) instead of `.diff-sel`. Keep the `wasRename`/`wasEdits`
   post-apply handling and the whole preview→apply→undo gate intact.
4. Replace the 7 `renderPreview(...)`/`showView("preview")` entry points with a
   new `enterDiffState()` that sets diff mode + renders the table + shows the bar.
   `discardPreview()`/`apply()`/`undo()` call an `exitDiffState()`.
5. Remove the **Preview** tab: `#view-preview` in `index.html`, its `showView`
   branch, and the `#preview-view` block (the `renderPreviewDiff` machinery can
   be deleted once nothing calls it). Keep `Files` + `Duplicates` tabs (slice 4).
6. **Verify hard** on disposable copies: every source (rename, move, edits,
   transform, cover embed/remove, clear, import) previews in-table, ticking a
   subset applies only those, Discard reverts staging, Apply writes + Undo
   restores — the safety gate MUST stay exact.

Transferable CSS: **A5 + A6** in `foundations/workspace.css`.

### Slice 4 — DEDUPLICATOR mode (design A7/A8, answer Q3)
- Add a 5th mode tab **DEDUPLICATOR** (`data-mode="deduplicator"`, `i-dedup`
  icon; compact-tabs already accommodates it).
- Move the dup **criterion + Scan** controls out of the `.dup-bar` into a right
  mode-panel `#panel-deduplicator` like every other mode.
- Render the grouped, **read-only** results in the main table area using the same
  `.files` shell: `tr.dup-group` rows with `.dup-badge` (N copies) + `.dup-key`,
  a per-file `.dup-note` keep-hint (`keep`/`smallest`/`byte-identical`), and an
  `i-lock` read-only banner (`.ro-note`).
- Remove the `Files | Preview | Duplicates` view strip entirely (Preview already
  gone in slice 3; this removes Duplicates + the Files tab too).
- Transferable CSS is **A7 + A8**.

## How to work here (project conventions — do not deviate)

- **Per-issue loop, autonomous:** file a GitHub issue → implement → verify on real
  behaviour (browser pane + on-disk checks, not just green tests) → `cargo fmt` /
  `clippy -D warnings` / `test` all green (for any Rust) → **signed** commit with
  `Closes #N` → add a `CHANGELOG.md` entry under `[Unreleased]` → push `main` →
  confirm the issue auto-closed. Slices 3+4 are expected to be **frontend-only**
  (`app/ui/*`), no Rust.
- **Git identity:** repo-local `ThaVip3r`, SSH-signed (Verified). **NEVER add the
  `Co-Authored-By: Claude` trailer.** Never name competitor tagger/DJ products.
- **Build & run:** `~/.cargo/bin/cargo tauri build --debug --bundles app`, then
  `pkill -f "target/debug/tagrex"; open target/debug/bundle/macos/TagRex.app`.
  The frontend is bundled into the `.app`, so rebuild after `app/ui/*` edits.
- **Verify the frontend** in the in-app Browser pane against
  `python3 -m http.server <port>` from `app/ui/` — **use a FRESH port every time**
  (the server sends no `Cache-Control`, so a reload serves stale CSS/JS). Drive
  via `javascript_tool`: top-level `function`s in `app.js` are global, and the
  mock at the bottom of `app.js` fakes every Tauri command. WKWebView ≠ Chromium
  (e.g. HTML5 DnD, and `resize`/`ResizeObserver` firing) — treat pane quirks as
  harness limits and confirm logic with manual calls.
- Test only on disposable copies in the scratchpad, never `~/Music`.

## Reference

- Brief: `docs/design/workspace-layout-iconography.md`
- Design deliverable: DesignSync `get_file` on `foundations/workspace.html` /
  `foundations/workspace.css` (project `b0312bbb-86f3-4e6a-a88c-06c8d9dc6e10`).
- Unrelated open item: **#114** (rework the ADD FIELD control) — not part of this
  redesign.

[#115]: https://github.com/tagrex/tagrex/issues/115
[#116]: https://github.com/tagrex/tagrex/issues/116
[#117]: https://github.com/tagrex/tagrex/issues/117
