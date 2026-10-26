# Design brief: one input-group component, and the mask builder's extract mode

> For Claude Design. The repo is linked locally, so **read the current code
> directly** rather than trusting this summary: `app/ui/index.html`,
> `app/ui/style.css`, and the mask builder round you just did
> (`foundations/mask-builder.*`, now in the app as `.mb-field` /
> `.mb-field-tools`). This brief is **intent only**; the component and where
> it applies are yours to decide.

## Part 1: one input-group component across the app

The mask builder's field, with its actions docked inside its border, was liked
at once. The app has several more fields with buttons beside them, and three
different takes on the same idea already exist:

- the table **filter** keeps its `.*` / `Aa` flags inside the box (`.filter-ctl`);
- the ONLINE **query** and the **library path** keep a caret menu inside
  (`.query`, `.lib`) while their other buttons sit outside;
- the mask builder's `.mb-field` docks icon actions inside.

Fields that still carry their buttons beside them: the RENAMER mask (`#mask`:
placeholder reference, mask presets, mask builder), the FROM NAME and FROM TEXT
masks (reference, presets), the column-mask pattern in the Add column dialog
(reference), the RENAMER destination (`#move-dest`: Choose…, back to library),
the ONLINE query's search/stop button, and the library path's browse and
re-read buttons.

Open questions:

1. **One component** for all of these (and the three existing takes): its
   anatomy, sizes for the two control tiers, separators, focus and error states,
   and how a growing (multi-line) field behaves.
2. **What goes inside and what stays outside.** An action that belongs to the
   field (insert, presets, clear, pick a value) versus the panel's main action
   (Search, Preview). The search/stop toggle and Choose… are the borderline
   cases.
3. **Discoverability.** Inside the border the actions are quieter; say how they
   stay findable (the user missed bare icons once, in the builder's earlier
   summary bar).

## Part 2: the mask builder in the extract direction

The builder today renders: tags in, a file name out, one row per file as
current name → new name. FROM NAME runs the same grammar the other way (the
file name in, tags out) and today shows that for the first selected file only.
The next step is the same builder for FROM NAME, and later FROM TEXT (a pasted
tracklist line per file).

Open question: **the result list in extract mode.** Instead of one target per
row it yields several fields per file (artist, title, track…), and the set of
fields depends on the mask. How should that list read: a table with a column
per extracted field and the file name first, row states for "the pattern does
not match this name" and for a value the transform presets changed, sideways
scrolling as in render mode. Keep it recognisably the same window as the render
mode.

## Deliverable

Preview cards for the input-group component (all its uses, light and dark) and
for the builder in extract mode, plus the CSS to fold back into
`app/ui/style.css`. Prefer extending existing classes over adding new ones.
