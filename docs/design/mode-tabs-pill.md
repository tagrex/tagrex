# Design brief: mode tabs as a segmented pill

> For Claude Design. The repo is linked locally, so **read the current code
> directly**: `app/ui/index.html` (`<nav class="mode-tabs">` inside
> `.topbar-main`) and `app/ui/style.css` (`/* ---- mode tabs ---- */`,
> `.topbar`, `.topbar-main`, `.brand`, `body.compact-tabs`). This brief is
> **intent only**; the answer is yours.

## The idea

The five modes (TAGGER, RENAMER, GENERATOR, DEDUPLICATOR, EXPORTER) are drawn
today as flat tabs with a 2px accent underline on the active one. The user
wants them to read as **one segmented control**: a rounded track with a
hairline border holding all five, each mode an icon plus its caps label, and
the active mode a softly tinted pill in the accent hue with accent-ink text
(the user attached a reference screenshot with the prompt). Nothing changes
in behaviour or markup: same buttons, same `data-mode`, same tooltips.

## Open questions

1. **Colours, both themes.** The track fill and border against the top bar
   (which is itself `--panel`), the active pill's tint, the hover state of an
   inactive mode, and the label/icon colours. There is no soft accent token
   yet (only `--accent` and `--accent-ink`): propose one (or say why not) with
   light and dark values, and check the active label still meets AA on it.
   Does the active pill need a shadow or a border of its own?
2. **Alignment in the row.** Row 1 of the top bar is: brand on the left
   (pushed right past the traffic lights on macOS,
   `body.mac-overlay-titlebar`), the tabs, a drag-region spacer, the update
   pill when there is one, then undo, settings, a hairline and the panel
   toggle at the far right. Should the pill stay left after the brand or sit
   centred in the row (centred on the window, or on the space left between
   brand and actions)? How should it hold up when the window narrows and
   `body.compact-tabs` drops the labels to icon-only?
3. **Height and rhythm.** The track adds its own inset; the row should not
   grow taller than it is now, and the pill's height should line up with the
   icon buttons on the right and the path field on row 2.
4. **States** already in the stylesheet that must survive: `:focus-visible`
   (`--ring`), `.mode-tab:disabled`, and the compact icon-only variant (round
   pills?).

## Deliverable

A preview card (light and dark) with the full top bar row in context: wide,
narrow (compact icon-only) and on macOS with the traffic lights; each state of
a mode (inactive, hover, active, focus, disabled) side by side; and the CSS to
replace the mode-tabs block in `app/ui/style.css` plus any new tokens, as in
the earlier rounds.
