# Design brief: button sizes, alignment and what they line up with

> For Claude Design. The repo is linked locally, so **read the current code
> directly**: `app/ui/index.html` and `app/ui/style.css`. The earlier rounds
> this follows are the cards `foundations/panel-actions`,
> `foundations/side-panel-layout`, `foundations/audit-1-controls`,
> `foundations/audit-2-edges`, `foundations/small-dialogs` and
> `foundations/spacing`. This brief is **intent only**; the answer is yours.

## Why now

The buttons follow three different rules, decided one panel at a time:

- **The side panels' closing row** (`.panel-actions`, #579): a grid of up to
  three equal cells; the run button fills the last cell, a secondary button
  keeps its own width at the start. The run buttons of every panel form one
  rail on the right, each as wide as a cell, whatever its label. This
  replaced the rule on your `panel-actions` card (the button as wide as its
  label, at the end of the last line).
- **The EDITOR footer** (`.editor-actions`, #862, just shipped): each button
  as wide as its label, Clear tags at the start, Write and next and Stage
  field changes as a pair at the end. In five of the seven languages the
  three do not fit the default 480px panel, so Clear tags sits alone on the
  first line and the pair wraps under it.
- **Dialogs** (`.modal-actions`): buttons as wide as their labels, at the end;
  the two-acts-and-cancel choice (#683) stacks them full width.

While checking #862 one button turned out 3px taller than its neighbours
(Clear tags lacked the class that sets `--ctl-h`); there may be others.

## Open questions

1. **Width**: as wide as the label, a shared minimum, or filling a cell or
   the row? One rule for the whole app, or one per kind of place (closing row
   of a panel, panel footer, dialog, preview bar, Settings)?
2. **Alignment inside a row**: where the primary goes, where its secondary
   goes (beside it, or at the other end), and where a destructive action
   (Clear tags) goes relative to the act that commits the work.
3. **What a row lines up with**: the panel's 14px content edge, the window
   edge, the status bar's right zone that sits under the panel (Preview),
   the rail of run buttons in the panels above. Should the button ends of
   the panel footer and the status bar line up?
4. **Windows other than the main one** (the transform preset editor, Settings,
   the small dialogs): the same rule, or their own? Does the platform matter
   (the order of OK and Cancel on macOS, Windows and Linux)?
5. **Long languages**: Russian, Ukrainian, German, French and Spanish labels
   run up to a third longer than English. What wraps first, and how, at the
   480px default, a wide panel and the 240px clamp?
6. **Height**: an audit of every text button against `--ctl-h`.

## Deliverable

A preview card (light and dark, English and Russian) showing the rule on the
EDITOR footer, one GENERATOR or RENAMER closing row, the preview bar, the status
bar under the panel, and one dialog, at 480px, a wide panel and the 240px
clamp; the CSS to fold back into `app/ui/style.css`, and the markup diff where
it has to change, as in the earlier rounds. Prefer extending `.panel-actions`,
`.modal-actions` and the existing button classes over new components.
