# Design brief — workspace layout, diff-state, iconography & cross-platform

> For Claude Design. The repo is linked locally, so **read the current code
> directly** rather than trusting this brief for markup:
> - `app/ui/index.html` — the top mode tabs (`#panel-tagger` / `#panel-renamer`
>   / `#panel-generator` / `#panel-exporter` and their `.mode-panel`s), the app
>   bar (`#browse`/`#open`/`#undo`/`#settings-open`/`#panel-toggle`), the left
>   **view strip** being removed (`#view-files`/`#view-preview`/`#view-duplicates`),
>   the `#files-view` table, the `#preview-view` diff (`.preview-bar`,
>   `#preview-diff`, `#show-old`, `#discard`, `#apply`), and the `#duplicates-view`
>   (`.dup-bar`, `#dup-criterion`, `#dup-scan`).
> - `app/ui/style.css` — `td.dirty`/`--dirty-tint` (inline dirty-cell tint that
>   already exists), the `.diff` diff-table styles, `button.icon` +
>   `button.icon > svg`, `.search-toggle` (the one existing inline-SVG icon),
>   `.filter-flag`, `.seg-btn`, `.text-btn`, `.col-menu`, and the tokens near the
>   top (`--accent`, `--ctl-h`, `--radius-*`, `--space-*`, `--font-*`).
> - `app/ui/app.js` — `showView`, `renderPreview`/`renderTracks`, the dup-scan
>   flow, and the glyphs injected dynamically (`▶`/`▼` group carets, `⋮⋮` grips,
>   `▲`/`▼` sort, `✕` delete).

## The decision this brief encodes

The current model is: top mode tabs (**TAGGER / RENAMER / GENERATOR / EXPORTER**)
drive a **right mode-panel** over a **shared left file table**, and the left area
carries a second-level **view strip** — **Files | Preview | Duplicates**. We've
decided that strip is the wrong seam and are dissolving it:

- **Files** is just "the table" — it doesn't need to be a tab.
- **Preview** only exists when a change is staged, and it duplicates information
  the table can carry. It becomes a **contextual diff-state of the table**, not a
  peer view.
- **Duplicates** is an *operation*, not a *view of the current selection*. It
  becomes a **top-level mode** alongside the others.

Target model, stated plainly: **top tabs = operations; the left area is always the
file table; the table enters a diff-state when a plan is staged.** Design should
mock **this** model — not a restyle of the old three-way strip.

## The ask

Three connected focuses; treat them as one system pass.

### 1. New information architecture (the core change)

**a. Dissolve the view strip.** Remove Files / Preview / Duplicates as sibling
tabs. The left column is the file table by default.

**b. Preview → a diff-state of the table.** When a plan is staged, the *same*
table shows the change in place rather than switching to a mirror diff view:
- Manual inline cell edits already tint dirty cells (`td.dirty`). For these,
  offer **Apply directly from the table** via a floating **Apply N · Discard**
  action bar — no mandatory detour. "Show as diff / Show old values" becomes an
  **optional** overlay, not a required stop.
- Batch plans (RENAMER, move, GENERATOR transform, online import, Clear tags,
  cover embed/remove) can't collapse to a single tinted cell — a rename changes
  the whole file path, and an import rewrites many fields across many files. So
  the diff-state must still preserve the three things the old Preview provided:
  the **old→new diff** (old value revealed under the changed one), **per-row
  apply selection** (tick which files to write), and the **"nothing is written
  until Apply"** gate. Design the in-table diff-state so these survive — e.g. a
  diff overlay on affected rows + the floating action bar + a way to show/scope
  which rows the plan touches (including path changes for renames/moves).

**c. Duplicates → a top-level DEDUPLICATOR mode.** Promote it to a mode tab next
to the others. To match every other mode's grammar, its **controls (criterion +
Scan) move into the right mode-panel**, and its grouped, read-only results render
in the main (left) area. Note: unlike the tag/rename/etc. modes it *replaces* the
plain table with a grouped result set — that main-area takeover is an accepted
exception; make it read as the same workspace, not a different app.
- Naming: `DEDUPLICATOR` keeps the -ER/-OR actor-noun pattern (TAGGER, RENAMER,
  GENERATOR, EXPORTER) but is long and will widen the tab bar; `DEDUPER` and
  `DUPLICATES` are shorter alternatives. Recommend a choice, especially in light
  of focus 2 (icons may let the tab bar breathe).

### 2. Iconography — glyphs → a single icon set

Today exactly one control uses a proper inline **SVG** icon (`#discogs-search`);
everything else is a **unicode/emoji glyph** in button text: `⚙` settings, `⇥`
panel collapse, `▶`/`■` player, `✕` close, `☰`/`▦` List/Grid, `▾` popover carets,
`⋮⋮` grips, `▲`/`▼` sort. The `button.icon > svg` plumbing already exists.

Deliver a small, coherent **inline-SVG icon set** (16px grid, single stroke
weight, `fill/stroke: currentColor` so it inherits theme + state) and a
per-control **icon / label / icon+label** mapping. Guideline to validate:
iconify frequent, repeated, space-competing, universally-legible controls
(Expand/Collapse, Columns, Presets, Undo, List/Grid, panel collapse, settings,
search); keep **text for consequential or ambiguous** actions (`Clear tags`,
`Stage field changes`, `Apply`, `Discard`, `Export`, `Scan library`,
`Browse…`/`Open`). Consider whether the **mode tabs** themselves read better as
icon+label — this is what buys back the width DEDUPLICATOR costs. Every icon-only
control needs `aria-label` + `title`.

### 3. Cross-platform consistency & portability

TagRex is Tauri, so the webview is **WKWebView on macOS, WebView2/Chromium on
Windows, WebKitGTK on Linux** — three engines, three default font stacks. Unicode
symbol glyphs diverge badly across them (`⚙` is a colour emoji on macOS, flat
elsewhere; `⇥`/`▦`/`☰` can render as **tofu** on minimal Linux font sets), with
metrics drift. The app is offline with a **null CSP**, so **no icon fonts, no
CDNs** — inline SVG or bundled assets only.
- **Retire unicode/emoji glyphs from every interactive control**, replaced by the
  focus-2 SVG set. This is the single biggest portability win.
- **Fonts:** specify a fully cross-platform UI sans stack (don't silently lean on
  macOS-only faces), and note the tie-in with the still-pending bundled condensed
  table face (`docs/design/table-condensed-font.md`). The bundled mono is the
  cross-platform model.
- **Native form chrome** — `<select>` (`#group-by`, `#online-source`,
  `#search-format`, `#query-preset`, and the dedup criterion), the
  `input[type=search]` clear affordance, scrollbars, and focus outlines look
  different per engine. Recommend a normalization pass and flag what must be
  verified on each engine.

## Constraints
- **Tokens only** — colour, spacing, radii, control height (`--ctl-h`),
  typography from the existing custom properties; must work in **light + dark**.
- **Offline, null CSP** — inline SVG or bundled assets only; no external
  CSS/JS/fonts/CDN.
- **Three engines** — WKWebView / WebView2 / WebKitGTK; no emoji or
  exotic-codepoint dependence in chrome.
- **Accessibility** — icon-only buttons carry `aria-label` + `title`; keep the
  focus-ring system; hit targets ≥ `--ctl-h`.
- **Preserve the safety model** — the preview→apply→undo gate is architectural
  (`docs/architecture.md`): the new diff-state must keep "nothing is written
  until Apply" and per-row apply selection. Don't design it away.
- **Reuse, don't reinvent** — build on `button.icon`, `.text-btn`, `.seg-btn`,
  `.col-menu` (shared popover shell), the filter toggles, and the existing
  `td.dirty` / `.diff` styling rather than new one-offs.

## Open design questions
1. In-table diff-state: how to show a **rename/move** (whole-path change) and a
   **multi-file import** diff inside the table without it becoming noise — and
   where the per-row apply checkboxes live in that state.
2. The floating **Apply / Discard** bar: where it sits (top of table, bottom, a
   pinned footer like the EDITOR actions), and how it coexists with grouping.
3. DEDUPLICATOR: main-area takeover vs. rendering results into the same table
   shell; and the final tab name given tab-bar width.
4. Mode tabs as **icon+label** vs text-only — does that read cleanly and buy the
   width back?
5. Styled vs native `<select>` for cross-engine consistency, without regressing
   accessibility.

## Deliverables
1. Mocks (light + dark) of the new workspace: the table as default, the table's
   **diff-state** with the Apply/Discard bar (show both an inline-edit case and a
   batch-plan/rename case), and the **DEDUPLICATOR** mode with its controls in the
   right panel + results in the main area.
2. The updated **mode tab bar** including DEDUPLICATOR (with the icon/label
   treatment you recommend).
3. The inline-SVG **icon set** (16px, currentColor) + per-control
   icon/label/icon+label mapping.
4. A short **cross-platform normalization** note (fonts, selects, scrollbars,
   focus) calling out what to verify on WKWebView / WebView2 / WebKitGTK.
