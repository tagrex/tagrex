# Design brief — condensed table font (#100)

> For Claude Design. The repo is linked locally, so **read the current code
> directly**: `app/ui/style.css` (the `@font-face` for `"TagRex Condensed"` and
> the `--font-ui-condensed` token near the top, plus
> `body.condensed-table #tracks tbody td`), `app/ui/index.html` (the Display
> settings group with `#set-condensed`), and `app/ui/app.js`
> (`applyCondensedTable`, `condensedTableEnabled`). The wiring is done — this
> brief is only the **face selection + subset production**.

## Context
The main file table (`#tracks`) renders in the bundled disambiguating monospace
(`TagRex Mono`, `assets/tagrex-mono.woff2`) so track numbers, filenames and tag
values align column-to-column. Monospace is wide, though, so long values truncate
early in a dense grid.

#100 adds an **optional** condensed sans for the table, toggled from
Settings › Display › "Condensed table font". The mechanism already ships:

- an `@font-face` slot `"TagRex Condensed"` pointing at
  `assets/tagrex-ui-condensed.woff2` (not yet present);
- a `--font-ui-condensed` token whose fallback stack uses a system condensed face
  (`Avenir Next Condensed`, etc.) until the bundled asset exists;
- a persisted toggle that swaps the table font.

So today the feature works via a **system** condensed font where the OS ships
one. This brief is to pick and produce the **bundled** face so it's consistent,
offline, and cross-platform.

## The ask
Deliver `assets/tagrex-ui-condensed.woff2` — a subset, single-weight condensed
sans-serif — dropped into the design project's assets like `tagrex-mono.woff2`
was.

## Requirements
1. **License:** open (OFL 1.1 or Apache-2.0), redistributable inside the app
   bundle.
2. **Width:** genuinely condensed/narrow so it packs more characters per column
   than the normal UI sans — that's the whole point. A semi-condensed face is
   acceptable if a full condensed one hurts legibility at small sizes.
3. **Legibility at 11–12px** (the table renders at `--text-2xs`, 10px, up to
   `--text-xs`). Validate on real filenames and tag values, not lorem.
4. **Cyrillic required.** Filenames and tags routinely contain Cyrillic, so the
   subset must cover **Latin + Latin-ext + Cyrillic + digits + the punctuation
   common in filenames** (`-_.,()[]&'!#%+ /` etc.). This makes the subset larger
   than the mono's — that's expected.
5. **Single weight (400)**, normal style. No italics, no ligatures needed.
6. **Small file** via subsetting (drop unused scripts/features), matching how the
   mono was produced.
7. **Digit disambiguation is a plus** (clear `0` vs `O`, `1` vs `l`) since the
   table shows years, track numbers, sizes — but not as strict as the mono, since
   this face trades alignment for density by design.

## Candidate families (open-licensed, with Cyrillic)
- **IBM Plex Sans Condensed** (OFL) — full Cyrillic, designed for UI.
- **Roboto Condensed** (Apache-2.0) — full Cyrillic, very legible small.
- **Fira Sans Condensed / Compressed** (OFL) — full Cyrillic.
- **Noto Sans Condensed** (OFL) — full Cyrillic, larger to subset.

Pick one, justify briefly against legibility at 11–12px with Cyrillic, and ship
the subset. No code changes needed — dropping the woff2 at the path above lights
it up ahead of the system fallback.
