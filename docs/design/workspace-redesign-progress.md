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
| 3 | **In-table diff-state** — dissolve the Preview view; the table shows the staged diff in place + a floating Apply/Discard bar; per-row apply moves to the sel column | A4, A5, A6 | _not filed_ | ⏳ NEXT |
| 4 | **DEDUPLICATOR** mode — promote Duplicates to a 5th top tab; controls into the right panel; grouped read-only results in the main area; view strip fully removed | A7, A8 | _not filed_ | ⏳ pending |

Both shipped slices are on `main`, signed, pushed, and bundled/relaunched.
The icon sprite already in `app/ui/index.html` includes `i-dedup`, `i-lock`,
`i-corner`, `i-arrow-right` — everything slices 3+4 need is present.

## What each remaining slice must do

### Slice 3 — in-table diff-state (design A5/A6, answers Q1+Q2)
- **Dissolve the Preview peer view.** Today `#preview-view` / `#view-preview` +
  `renderPreview()` build a separate `.diff` mirror table in `#preview-diff`, and
  `apply()` reads ticked rows from `.diff-sel:checked` there. Rework so a staged
  `previewPlan` renders **into the main `#tracks` table** instead.
- **Diff-state shapes:** (a) manual inline edits already tint `td.dirty` — just
  surface the floating bar and Apply directly; (b) batch plans (rename/move/
  import/transform/clear/cover) mark whole rows: `tr.staged` (accent left-bar on
  the sel cell), `tr.untouched` recedes, changed cells reuse `td.dirty`/
  `td.error`; a rename shows the new name in the File cell and a reorganize adds
  the new relative path (`.fcell`/`.fname`/`.fpath` led by the `i-corner` icon);
  old→new revealed via `.show-old` + `.cell-old`.
- **Per-row apply lives in the sel column** (made visible in diff-state); ticking
  scopes exactly which files Apply writes — this is how the preview→apply→undo
  gate + per-row selection survive. Do NOT design the gate away.
- **Floating Apply/Discard bar** `.diff-actionbar`: pinned bottom-centre of the
  table pane, carries the count/plan-name, an optional **Show old values**
  toggle, and Discard/Apply. Coexists with grouping; never steals a table row.
- Transferable CSS for all of this is **A5 + A6** in `foundations/workspace.css`.

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
