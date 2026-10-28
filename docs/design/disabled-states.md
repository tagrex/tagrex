# Design brief: how unavailable controls look (the inert stripes)

> For Claude Design. The repo is linked locally, so **read the current code
> directly**: `app/ui/style.css` (search for `--inert-stripes`) and
> `app/ui/index.html`. The rule comes from your States round
> (`foundations/states.*`, `foundations/inert.*`). This brief is **intent
> only**; the answer is yours.

## The question

A disabled control is drawn with the soft diagonal stripes
(`--inert-stripes-soft`) and muted text: `button:disabled`, `.seg-btn:disabled`,
disabled text, search and password fields, `select:disabled` (with its caret
over the stripes, #552). `button.primary:disabled` loses its green and becomes
the same striped grey bar.

Looked at in the real app, this reads less well than it did on a card:

- **It is not what any platform does.** macOS, Windows, GNOME and Material all
  draw an unavailable control as itself, dimmed: same shape, faded fill, pale
  text. Stripes on a button are a pattern people know from loading bars and
  "under construction".
- **The app uses the same stripes for loading.** The skeleton loader
  (`.skeleton`, `.skeleton-line`) is these stripes, sliding. A disabled button
  and a loading placeholder are one texture apart by motion only.
- **A disabled primary stops reading as the primary action.** The run button
  of a job becomes a grey striped bar the same as any other disabled button,
  so the panel loses its one strong anchor (seen on the preset editor's
  Delete, the mask builder's Use this mask and, until #595, RENAMER's Preview).

Where the stripes do work: an area that holds nothing, which is what the
"inert" motif was for. The cover well with no selection (`.cover-thumb.inert`),
the release card's missing art (`.release-cover`), an unreadable row
(`tr.unreadable`), `.inert` itself. Empty states already dropped the fill for
a dashed edge (#245).

## Open questions

1. How a **disabled button** reads: the platforms' dimmed control, or
   something else in the house style. Separately for `.primary`, a plain
   button, `.btn`, an `.icon` button and a `.seg-btn`.
2. How a **disabled field and select** read (the select keeps its caret).
3. Whether the stripes stay for the **inert areas** above, and for the
   skeleton loader, or the two should be told apart.
4. Read-only versus disabled: a built-in preset's rules are read-only, drawn as
   values since #556; check that nothing in the new rule pulls them back to
   looking disabled.

## Deliverable

A preview card (light and dark) with each kind of control enabled, hovered and
disabled side by side, a panel with a disabled primary in context, and the
inert areas; the CSS to replace the disabled block in `app/ui/style.css`
(`button:disabled, .seg-btn:disabled, input…:disabled, select:disabled` and
`button.primary:disabled`), as in the earlier rounds. Check contrast: a
disabled label still has to be legible, though it need not meet AA.
