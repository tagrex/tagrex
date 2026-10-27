# Design brief: the run buttons and the free space around them in the side panels

> For Claude Design. The repo is linked locally, so **read the current code
> directly** rather than trusting this summary: `app/ui/index.html`,
> `app/ui/style.css`. Your earlier rounds are the reference for the house
> style, above all `foundations/generator.*`, `foundations/exporter.*` and
> `foundations/workspace.*`. This brief is **intent only**; the answer is yours.

## The problem

Every side-panel job ends with its run button on a line of its own, pushed to
the right by a spacer (`.panel-actions` with `.spacer`). Whatever sits above it
is laid out on its own terms, so the free space wanders: sometimes on the
right of a column of checkboxes, sometimes on the left of the button,
sometimes both.

The clearest case is GENERATOR (`#panel-generator`, 480px wide):

- **Number tracks**: a row with Start at and Disc #, then three checkboxes
  stacked in a column (Track total, Restart per group, Keep vinyl sides)
  leaving the right half empty, then the Number tracks button alone on its
  line with the left two thirds empty.
- **Vinyl sides**: two lines of explanation, then Split side → disc alone on
  its line.
- **Transform**: Edit presets… on the left and Preview changes on the right
  share a line, which reads well.

The same pattern is in RENAMER (Preview rename under two checkboxes), EXPORTER
(Export under the note), FROM NAME and FROM TEXT (their Show tags buttons) and
DEDUPLICATOR. The panel column is resizable (`--panel-col-width`), so a fix has
to hold from the narrow default up to a wide panel.

Context: since #568 the GENERATOR panel fills the column and Transform takes
the spare height, so Number tracks and Vinyl sides sit at the bottom.

## Open questions

1. **Where the run button goes** relative to the controls it runs: its own
   line at the end (today), beside the last row of controls, or something
   else. One rule for every panel?
2. **Groups of checkboxes**: a column, a row that wraps, or beside the
   button?
3. **A job with no controls** (Vinyl sides is text plus a button): how it
   reads without a lone button under a paragraph.
4. Whether the explanation paragraphs under each heading stay as they are,
   or shorten the way the mask hints did in round 2 of the last brief.

## Deliverable

A preview card (light and dark) with GENERATOR at the default width and a
wide one, plus one other panel to show the rule carries; the CSS to fold back
into `app/ui/style.css`, and the markup diff where it has to change, as in
the earlier rounds. Prefer extending `.panel-block` / `.panel-actions` over new
components.
