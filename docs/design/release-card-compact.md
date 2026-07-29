# Design brief — compact release card (#98)

> For Claude Design. The repo is linked locally, so **read the current code
> directly**: `app/ui/app.js` (`cardMarkup`, `renderTracklist`, `mediaBadgeMarkup`,
> `countLabel`), `app/ui/style.css` (`.release-card`, `.release-head`,
> `.release-cover`, `.release-info`, `.release-line1`, `.release-artist`,
> `.release-title`, `.release-meta`, `.media-badge`, `.release-tracklist`,
> `.tk-num/.tk-title/.tk-artist/.tk-dur`, `.tracklist-label`,
> `.tracklist-apply`), and `app/ui/index.html` (`#release-list`). Intent only —
> it does not restate the markup.

## Context
The TAGGER › ONLINE panel lists online search results as **release cards** in a
List view (there's also a Grid). A card has a collapsed header and expands to a
tracklist; import is per-card. The header was recently reworked into four lines
(catalogue no. + track/disc count · album artist · album title · country · year ·
format), with a media-type badge on a 64px cover and a right-side expand caret.
It reads well but is **taller and less dense than it could be**. This pass makes
the card **more compact and more information-dense while keeping our aesthetic** —
explicitly "our look, their density".

The panel is narrow (~480px) and the tracklist columns are **not user-resizable**,
so horizontal budget is tight.

## Focus
1. **Cover fills the full height of the collapsed header.** Today it's a small
   64px thumbnail beside the text; make the cover span the header's full height so
   the card is more image-forward, with the four text lines sitting compactly
   beside a taller cover. Keep the media badge (List only).
2. **Higher info density, minimal header height.** Fit catalogue no. + track/disc
   count, album artist, album title, and country · year · format at the smallest
   comfortable height without losing scannability. Decide what shares a line.
3. **Tracklist as a tight table with minimal row height.** Combine **title +
   artist into one cell** (they're separate columns now), keep a leading
   **checkbox + track number** group and a trailing **duration**. Squeeze row
   height to the minimum that still reads.
4. Preserve the existing affordances: List/Grid toggle, the label · cat# picker
   (shown when a release lists more than one pair), "Import to selected files",
   the expand caret, per-track checkboxes.

## Constraints
- **Light + dark**, both correct (theme-aware app).
- Reuse the existing design system — tokens, `.seg`/`.seg-btn`, `button.primary`,
  the `.catno`/`.pill` chips (both bundled-monospace), `.media-badge`. Do **not**
  invent new colours/spacing/type.
- Legible at 11–12px incl. **Cyrillic** (tags/filenames use it).
- Self-contained, offline/CSP-safe: vanilla HTML/CSS, no external assets.
- Must fit the ~480px panel; nothing relies on user-resizable columns.

## Questions to answer in the deliverable
- **Q1** Full-height cover: how tall is the collapsed header, and how do the four
  text lines lay out beside a taller cover (which lines pair up)?
- **Q2** Density: the minimal-height arrangement of catno + count + artist +
  title + meta that stays scannable at a glance.
- **Q3** Tracklist row: title+artist combined (inline vs two-line stack), the
  minimal row height, and where the checkbox+number and duration sit.
- **Q4** Continuity: how the compact header visually flows into the expanded
  tracklist.

## Deliverable (into the "TagRex Design System" project)
Self-contained preview HTML/CSS (`foundations/release-card-compact.html` +
`release-card-compact.css`) showing the **collapsed** and **expanded** states,
light + dark, on our tokens, each with a first-line
`<!-- @dsCard group="RELEASE-CARD" … -->` marker. A short spec with the Q1–Q4
answers. No interactive `.dc.html` for integration.
