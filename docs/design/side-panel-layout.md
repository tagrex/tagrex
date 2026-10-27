# Design brief: composing the side panel (round 2 of the panel actions)

> For Claude Design. The repo is linked locally, so **read the current code
> directly** rather than trusting this summary: `app/ui/index.html`,
> `app/ui/style.css`. The previous round is `foundations/panel-actions.*` and its
> brief `docs/design/panel-actions.md`; the house style is in
> `foundations/generator.*`, `foundations/exporter.*` and
> `foundations/workspace.*`. This brief is **intent only**; the answer is yours.

## Why a second round

The first round gave one rule for the run button (it closes the block's last
line, beside the job's flags or note). Folded into the app and looked at in the
real window, the panels still read as loose: each fix moved the free space
somewhere else instead of giving it a place. The question is now the panel as a
whole, not the button.

## What is there now (GENERATOR, `#panel-generator`, the clearest case)

Three jobs stacked in a 480px side column (resizable, `--panel-col-width`), the
window from about 900px tall up to a full screen:

- **Transform**: a hint, the ticked-preset checklist (`#groups-menu`), then
  Edit presets… and Preview changes on one line. Since #568 the panel fills the
  column and the checklist takes the spare height. With the ten built-in
  presets and none of your own, that leaves an empty framed area at the bottom
  of the list, a third of the list's height.
- **Number tracks**: a hint, Start at and Disc # stacked on the left (sized to
  their one to three digits), the three checkboxes in a column to their right,
  and Number tracks on its own line below, at the right. The right third beside
  the checkboxes and the left of the button's line are empty.
- **Vinyl sides**: a two-line note with Split side → disc beside it.

## What was tried, and why it did not hold

- Run button alone on its own line at the right, everything above full width
  (the original): free space on the left of every button.
- Button at the end of the flags' line (#571): with the fields then sized to
  their content (#573), the corner above the button went empty.
- Fields and flags as one block, button centred beside it (#576): the corner
  above the button was still empty.
- Fields stacked, flags in a column beside them, button below (#577, current):
  the right third and the button's line are empty.
- Checklist stretched to the spare height (#568): an empty framed area when the
  list is short.

## Open questions

1. **The panel as a composition**: one variable-height job (Transform) and two
   fixed small ones. Where does the free height go: into the list, between the
   jobs, below the last one, or nowhere (the jobs packed at the top)?
2. **A small job's internal layout** (Number tracks: two numeric fields, three
   checkboxes, a button): one arrangement that holds from the 480px default to
   a wide column, without a lone button or an empty corner.
3. **The checklist**: does it grow with the window, and if so, how does it read
   when it holds fewer rows than its height?
4. **One rule for every side panel**: RENAMER, EXPORTER, DEDUPLICATOR and the
   TAGGER panels (FROM NAME / FROM TEXT have their run button in a pinned
   footer, `.from-name-actions`). Should the side panel's run buttons move to a
   footer like that everywhere, or stay inside each job?

## Deliverable

A preview card (light and dark) with GENERATOR at the default width, at a wide
one, and in a short and a tall window, plus one other panel to show the rule
carries; the CSS to fold back into `app/ui/style.css` and the markup diff where
it has to change, as in the earlier rounds. Prefer extending `.panel-block`,
`.panel-actions` and the existing panel structure over new components.
