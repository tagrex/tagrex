# Design brief — List-view release card: media-type disc masks & badges (#98)

> For Claude Design. The repo is linked locally, so **read the current code
> directly**: `app/ui/app.js` (`cardMarkup`, `discCount`, `candidateMeta`),
> `app/ui/style.css` (`.release-card`, `.release-cover`, `.release-info`,
> `.release-line1`, `.catno`, `.pill`), and `app/ui/index.html` (`#release-list`).
> Intent only — it does not restate the markup.

## Context
The TAGGER › ONLINE panel shows online search results as **release cards** in a
List view. The text layout was just reworked into four lines (catalogue no. +
track/disc count, then album artist, then album title, then `country · year ·
format`), with a **56px square cover thumbnail** on the left (striped placeholder
until the image loads). That text part ships as-is. This brief is **only** the
visual media-type treatment, which is a design call, not a code one.

## The ask
Show the release's **media type and disc count** on/around the cover thumbnail so
a DJ reads format and set size at a glance without parsing line 1. Explore two
directions (pick one, or propose better) on the existing design system — reuse
the tokens/components already in `style.css`; do not invent new colours:

1. **Peeking-media mask** — a vinyl or CD silhouette sliding out from behind the
   cover to one side; multi-disc sets add another peeking disc per extra disc
   (cap the visible stack, e.g. 3, then "+N").
2. **Corner badge** — a small overlay marking media type (vinyl / CD / digital)
   and, when >1, the disc count.

## Inputs per card (already available)
- `format`: free provider text (`CD, Compilation, Mixed`, `Vinyl, LP`, `File,
  FLAC`). Media type is inferred from it — the deliverable should state the
  keyword→kind mapping it assumes.
- `discCount`: integer ≥1 (already computed from track positions).
- Cover thumb is 56×56 and may be missing (striped placeholder must still read).

## Focus / constraints
- **Light + dark**, both correct. Theme-aware app.
- Must not obscure the catalogue-number chip or bury the art.
- Legible at 56px (List); should degrade gracefully if reused in the larger Grid
  tile (`.release-tile`/`.tile-cover`).
- **Offline / CSP**: inline SVG or CSS masks only — no external images, no web
  fonts beyond the bundled mono.

## Questions to answer in the deliverable
- **Q1** Mask vs badge — which reads faster at 56px, and why?
- **Q2** Multi-disc: how the stack grows and where it caps (2 / 3 / 5+ discs).
- **Q3** Media inference: the `format`-keyword → vinyl/cd/digital/generic mapping.
- **Q4** Missing-cover state: how the mask/badge sits over the striped placeholder.

## Deliverable (into the "TagRex Design System" project)
Self-contained preview HTML/CSS (`foundations/release-media.html` +
`release-media.css`) covering the states — 1 / 2 / 3+ discs; vinyl / CD /
digital; light + dark; cover present / missing — each with a first-line
`<!-- @dsCard group="RELEASE-CARD" … -->` marker, on our tokens. A short spec
with the Q1–Q4 answers. No interactive `.dc.html` for integration.
