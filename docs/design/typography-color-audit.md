# Design brief — typography audit + color/appearance pass

> For Claude Design. The repo is linked locally, so **read the current code
> directly** rather than trusting this brief's summary: `app/ui/style.css` (the
> `@font-face` blocks and the `:root` token stacks near the top — families,
> scale, weights — plus every `font-family:` use), `app/ui/index.html`, and the
> two colour palettes (`:root` light + the dark `@media`/`[data-theme="dark"]`
> blocks). This brief is **intent only** — the audit and the recommendation are
> yours.

## Goal
Audit and rationalize the app's **type system** and take a second look at the
**colour palette / overall appearance**. Two loosely-coupled deliverables in one
round; the type work is the priority.

## Part 1 — Typography (priority)

Three things to get right, in order:

1. **Shrink the font zoo.** Today the UI leans on several distinct family
   tokens — a system UI sans, a system mono, a bundled disambiguating mono, and
   a (not-yet-bundled) condensed sans. Decide how few faces this app actually
   needs and recommend a consolidated set. Fewer, harmonious faces beat a wide
   stack. Note: a bundled condensed face is **already speced separately** in
   `docs/design/table-condensed-font.md` — fold that decision into this one
   (ideally pick faces that share a design language) rather than choosing in
   isolation.

2. **Cross-platform portability.** Text must render consistently and correctly on
   **macOS, Linux, and Windows**. The current stacks are uneven — e.g. the UI
   sans stack has no real Linux fallback before the generic, and the condensed
   stack is built from OS-specific faces that exist on only one platform each.
   Two acceptable strategies, your call which fits where:
   - **Bundle** an open-licensed face (OFL/Apache) subset to a woff2, the way the
     mono already is — identical rendering everywhere, offline, at the cost of
     bundle weight and native feel; or
   - **Per-OS system stacks** done properly — a genuinely complete fallback chain
     for each of the three OSes (macOS / Windows / common Linux desktop faces)
     so no platform silently drops to an ugly generic.
   Recommend per-role (UI text vs mono vs condensed) which strategy wins.

3. **Legibility + harmony.** Every text element (dense table, toolbars, labels,
   headings, chips, mono values) should read cleanly at its size — the scale runs
   small (10–15px, see the `--text-*` tokens) — and the faces should look like
   one family, not a collage. **Cyrillic coverage is mandatory** for any bundled
   face: filenames and tag values routinely contain Cyrillic. Digit/glyph
   disambiguation (`0` vs `O`, `1` vs `l`) still matters for the mono.

### Superfamily candidates (a steer, not a mandate)

The cleanest way to satisfy "few faces + harmonious + full Cyrillic" is one
open-licensed **superfamily** whose sans / mono / condensed members share a
design language, so mixing them reads as one system. Consider, but you're free
to propose better:

- **IBM Plex** (OFL) — Plex Sans + Plex Mono + Plex Sans Condensed, all with full
  Cyrillic and one common skeleton. Strong default candidate.
- **Noto Sans + Noto Sans Mono + Noto Sans Condensed** (OFL) — full Cyrillic,
  very complete, but heavier to subset.
- **Fira Sans / Fira Mono / Fira Sans Condensed** (OFL) — full Cyrillic, good at
  small sizes; the mono is a proven disambiguator.
- **Source Sans 3 + Source Code Pro** (OFL) — no first-party condensed, so it'd
  need a condensed from elsewhere; list only if the sans/mono pair wins on merit.

Whatever you pick: justify it briefly against small-size legibility with Cyrillic
and against the current bundled mono (`TagRex Mono` = JetBrains Mono subset) —
say whether to keep that mono or switch to the superfamily's mono for coherence.

If you recommend bundling any face, deliver the subset woff2 into the project
assets (Latin + Latin-ext + Cyrillic + digits + filename punctuation), same as
`assets/tagrex-mono.woff2` was.

## Part 2 — Colour & appearance (secondary)

A lighter review of the palette and general look:

- **Contrast / accessibility:** check text and UI colours against WCAG AA in both
  light and dark, especially small text, the muted grey, accent-as-text
  (`--accent-ink`), and diff add/del colours.
- **Green-on-green risk:** the brand accent and the "add"/success colour are both
  greens (`--accent` vs `--add`) and the tinted row/selection states are also
  accent-green. Flag anywhere the accent and a success/diff signal could be
  confused, and propose a fix if real.
- **Light/dark parity:** the two palettes are hand-duplicated; note any drift or
  pairs that read very differently between themes.
- Any small, high-leverage appearance tweaks you'd make while here (spacing,
  radii, borders, elevation) — as suggestions, not a redesign.

Keep the accent anchored to the brand green (logo `#085041`); this is a tune-up,
not a rebrand.

## Deliverables
- A short written recommendation: the consolidated font set, per-role
  bundle-vs-system strategy, and the colour findings.
- Any bundled face(s) as subset woff2 in the project assets.
- Preview/spec cards showing the type scale and the palette (light + dark) so the
  result is reviewable in the Design System pane.
