# Design brief: the mask builder window in the house style

> For Claude Design. The repo is linked locally, so **read the current code
> directly** rather than trusting this summary: the `#mask-builder` dialog in
> `app/ui/index.html`, its styles (`.mask-builder` and the `.mb-*` rules at the
> end of `app/ui/style.css`), and `app/ui/js/maskbuilder.js`. Compare it with
> the dialogs and panels that are already designed: the preset editor
> (`#pe-editor`), the Transform window (`#transform-modal`), the RENAMER panel's
> mask row, and the Settings slide-over. This brief is **intent only**; the
> layout and the choice of components are yours.

## Goal

The mask builder (issues #540 to #545) was composed in code, one round of
feedback at a time, and it shows: the window works, but its controls do not
look like the rest of the app. Lay it out with the components and tokens the
system already has, so it reads as one of the app's own dialogs.

## What the window holds

- A **mask field**, one line that grows to five as the mask wraps.
- Three **actions on that field**: open the mask presets popover, open the
  Transform window (with the number of rules that will run), and clear the field.
- A one-line **error** under the field when the expression does not parse.
- A one-line **summary** ("4 new names · 1 clashing · incl. 6 rules").
- The **result list**: every selected file as current name → new name, one line
  each, scrolling sideways when long. Rows have three states besides a plain
  change: unchanged, a required placeholder empty for that file, and a name
  clashing with another file.
- The **placeholder and function reference** on the right, with a filter box,
  always open; a click inserts at the caret.
- **Cancel** and **Use this mask**; the window is resizable from its corner.

## Open questions

1. **The three field actions.** Which of the system's existing button kinds do
   they belong to? Today they are the compact `.btn-sm` from the release card,
   which looks foreign here. As bare icons beside the field they made the field
   narrower than the list; as icons in the summary bar they went unnoticed.
2. **Where they sit** in the composition, so they are found without breaking
   the column (field and list share edges today, which the user liked).
3. **Alignment across the two columns**: the field against the reference's
   filter box, the list against the reference's bottom, and the footer.
4. **Row states** in the list, consistent with the in-table diff state and the
   rest of the palette, in both themes.

## Deliverable

A preview card for the window (`foundations/mask-builder.html` + `.css`) in
light and dark, plus the CSS to fold back into `app/ui/style.css`. Prefer
reusing existing classes over new ones.
