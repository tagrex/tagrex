# Visual system

TagRex is a dense desktop workbench. Shared rules make it easier to scan without
changing its familiar layout, table density or preview → Apply → Undo workflow.
The surface palettes and saved accent remain independent of typography.

## Ownership

`style.css` owns visual role tokens. `shadcn.css` maps component utilities to those
same runtime tokens. `form-controls.css` owns field/focus metrics; `workbench.css`
owns settings and common legacy controls; `workbench-system.css` owns the shell,
table chrome, player and action footer. Inspector, filter, docking, preset and
media styles own their respective surfaces. `surfaces.css` owns controller popup
shells and list rows. Change a rule at its owner rather than adding an override
stylesheet or another copy of a token scale.

## Spacing and grouping

Use `--sp-1…7`: **4, 8, 12, 16, 24, 32, 48px**. Related labels/help use 4px;
control clusters use 8–12px; task groups use 16px; separate sections use 24–32px.
Inspector fields share a left-hand label rail and an 8px vertical gap. Compact
fields reuse that row layout. Track/Disc, Year/Compilation and BPM/Key keep
their desktop rows at every dock width; input text is left aligned and labels
are vertically centred. The everyday fields are always visible. Keep the form
at a usable minimum width inside its own scrollport rather than stacking fields
or moving labels above them. Keep gaps on the parent so paired
rows do not lose their separation from ordinary rows.
A parent gap and child margin must not both own the same separation. Optical
1–2px adjustments, border-inclusive menu insets, splitters and typography-derived
data-row pitch are deliberate exceptions. Legacy `--space-*` values are retained
only for compatible dense layouts until their individual owners are migrated;
do not globally remap them and alter table geometry.

Normal controls use 32px, compact controls 28px, icon hit areas at least 24px.
A glyph can be smaller than its hit area. Dense table cells, checkbox labels and
splitter hit bands have their own interaction contracts. Keep wrapping controls
and scrolling content reachable in narrow or short windows.

## Corners and focus

| Role | Token | Radius |
| --- | --- | ---: |
| Input/select/compound field | `--r-control` | 10px |
| Compact command or tab | `--r-compact` | 8px |
| Recessed section | `--r-inset` | 14px |
| Menu shell | `--r-menu` | 16px |
| Menu row | `--r-menu-item` | 8px |
| Dialog / narrow dialog | `--r-dialog` / `--r-dialog-compact` | 22 / 18px |
| Standalone action / circular glyph control | `--r-pill` / `--r-circle` | pill / circle |
| Media viewport / image edge | `--r-media` / `--r-media-edge` | 8 / 2px |

For adjacent parallel contours, subtract the border-box inset from the outer
radius. A menu's 1px border plus 7px padding gives an 8px inset: 16px shell,
8px row. A flush input/dirty tint shares the input's 10px contour. Independent
artwork and separate tonal planes are not obligated to follow that formula.
Table cells and continuous dock interiors stay square; the enclosing file pane
uses the 14px inset contour. Prominent standalone actions use pills. Quiet text
commands and palette choices keep compact contours; menu rows and field locks
do not inherit action geometry by default.

A compound field owns one focus outline. Keep its text input and tools inside the
same exposed contour, with space for keyboard focus. Invalid, disabled and active
states retain geometry. Popup bodies scroll inside a bounded shell while header,
footer and first/last focus targets remain reachable.

## Typography and fonts

The interface offers **Inter** (default), **IBM Plex Sans**, **Source Sans 3** and
**JetBrains Mono**. All are bundled offline with font licences and reproducible
source provenance. Store the choice separately from the existing data-font,
condensed, badge and table/tracklist-size preferences. Appearance choices apply
and persist immediately, including when Settings is dismissed with Cancel.

`--font-ui` follows the interface preference. `--font-value` follows the data-font
choice; its legacy `sans` value means Interface. `--font-literal` always uses the
bundled monospace for masks, regex and literal syntax. Keep the old Mono and
Condensed data choices and existing size ranges compatible. SVG icons remain
independent of the selected face.

| Role | Size at 16px root | Weight |
| --- | ---: | ---: |
| Large destination | 20px; Settings section 22px | 600 |
| Compact dialog/section | 15px | 600 |
| Inspector heading | 14px | 600 |
| Body, editable values | 13px | 400 |
| Controls/group labels | 13px | 500 |
| Help and secondary labels | 12px | 400 |
| Short secondary metadata | 11px | 400 |

Use shared rem-based UI roles and unitless leading; retain independent data-size
preferences. Real 500/600 outlines carry hierarchy. Headings use primary ink,
labels the label role (the root fallback follows muted ink; dark palettes
strengthen it for separation), help muted ink and placeholders quiet label/muted
ink. Ordinary text and placeholders retain at least 4.5:1 contrast. Reserve
italics for actual emphasis; mixed/empty placeholders remain plain and muted.
Sentence case is translated copy, not a CSS transform over uppercase strings.

## Verification

Check all modes/subtabs, settings groups, dialogs, menus, artwork, filters,
player, progress/toasts and empty/error/locked/staged states. Sample every
interface face, existing data preference, both themes, palette families,
long/localized text, narrow docks and short viewports. Preserve raw drafts,
selection and focus. Table virtualization and footer/player alignment remain
behavioral gates, not styling details. New fonts require verified weight/glyph
coverage, internal naming, licence notices and source hashes; rebuild them with
`tools/build-ui-fonts.py` using its pinned requirements/provenance.

Browser mocks prove their exercised UI scope. Native dialogs, media writes,
platform rasterization and OS scaling require separate evidence before claims.

## Pending work

Keep one compact recessed **Staged edits** summary pinned at the inspector's
bottom, aligned with the table bottom. It appears only while the actual plan
contains changes and shows the unique file count plus an × to clear the draft.
Clear never erases file tags. Preview and Apply sit together in the footer;
Apply is the sole primary save action and names its actual file scope.

Files and Staged use the same virtualized table, columns and grouping. Compact
browser-style tabs have a darker inactive fill; the active tab joins the rounded
folder bar, with an 8px gap before the table. Files retains its ordinary readable
rows. Staged lists every pending file regardless of the Files filter, keeps
old/new comparison on, and dims only unchanged cells. Switching tabs preserves
selection, drafts and each view's scroll position. Returning a staged tag cell
to its on-disk value removes that proposal after editing finishes, so the row
does not disappear mid-keystroke. Tag-only plans retain normal cell editing;
explicit rename/artwork/block previews retain their existing review safeguards.

Empty selection leaves the inspector blank while work already staged on other
files remains available. The everyday metadata form retains its desktop rows,
left labels and left-aligned values as the dock narrows. Tag-block inspection
and conversion belong in file context actions, not a permanent line above the
fields. Text-tag clearing stays in the context menu/More actions.

## Popup behaviour

- A field choice list attaches to the **whole field**, with the same position for
  mouse, label activation and keyboard. It is at least the field width (or 160px
  for compact controls), bounded by the viewport. Selected options keep a check.
- A button action menu opens 4px below its button; trailing three-dot actions
  align their right edges. It flips above when needed. Rule pickers and pattern
  tools follow this rule, rather than the precise point clicked inside them.
- A right-click menu opens at the pointer. Keyboard context menus attach to the
  focused target. Screen edges can move either kind inward, with an 8px gutter.
- Menus share the 16px shell / 8px item radius, 7px shell padding plus 1px border,
  32px basic row, left-aligned text, quiet descriptions and opaque palette fill.
  Longer descriptions may grow rows; form/help popovers retain their own layout.
- Only popup content scrolls when it is too tall. Scrolling within it preserves
  the menu. Anchored controller popups follow their owner as its panel scrolls,
  and close if the owner is hidden/clipped; point menus dismiss on outside wheel
  scrolling or resize. Escape closes the current menu and returns focus to its
  owner; choosing a native-field option still emits one native change event.

`AnimatedSelect.svelte` owns native-field proxy semantics. Framework primitives
own their trigger positioning and collision gutters. `js/popup-position.js`
owns viewport and transformed-dialog placement for imperative controllers;
`dom.js` re-exports its API. Do not add cursor anchors to field selectors or
reimplement positioning separately for each action menu.
