# Design brief: an audit of the whole interface (alignment, layout, sizes, type)

> For Claude Design. The repo is linked locally, so **read the current code
> directly**: `app/ui/index.html`, `app/ui/style.css` and the modules in
> `app/ui/js/` that build markup at runtime. The house style is what the earlier
> rounds settled (`foundations/*` in the design system, the briefs in
> `docs/design/`). This brief is **intent only**: find what is off, rank it,
> and say how to fix it within that style.

## Why

The app grew one feature at a time, and the last rounds redesigned parts of it
(the mask builder, the input group, the windows, the side panels). Each part
was checked on its own; nobody has looked at the whole for consistency since.
Small faults keep turning up one at a time in the real app: a chip sitting
below its line, a field twice as wide as its value, a gap twice the usual
size, a label in lower case among capitalised ones. This round looks for all
of them at once.

## What to look for

1. **Alignment**: edges that should line up and don't (a field's left edge and
   the content below it, the right edges of a panel's controls and the top
   bar's), controls on one line that don't share a centre, text baselines
   across neighbouring controls, icons off-centre in their buttons.
2. **Layout and spacing**: gaps that differ for no reason between siblings,
   spacing not on the `--space-*` scale, free space that collects in odd
   places, blocks that don't use the house patterns (`.panel-block`,
   `.panel-actions`, `.modal-head`, `.field`).
3. **Sizes**: control heights off `--ctl-h` / `--ctl-h-sm`, buttons of one kind
   at different sizes, icons at sizes other than the set ones, fields far wider
   or narrower than their content, hit targets that are too small.
4. **Typography**: sizes off the `--text-*` scale, weights and small caps used
   inconsistently for the same role, sentence case versus capitalised labels,
   mono versus sans for values, line heights, truncation and wrapping (an
   ellipsis where a value matters, a label that wraps where it shouldn't).
5. **The conditions things break under**: the light and the dark theme; the
   longest labels (Russian and German run 30 to 60% longer than English); the
   side column from the 480px default to wide; a short window (about 900px)
   and a tall one; macOS (`body.platform-mac.close-start`, traffic lights in
   our top bar) as well as Windows and Linux.

## Where (every surface)

- **Top bar**: the brand, mode tabs, undo, settings, panel toggle; the library
  path field with its recent menu and refresh.
- **Table**: toolbar (filter field, filter presets, health, columns, wand,
  BPM, clear tags), header, folder bands, rows and their states, the diff
  state's cells and the floating Apply / Discard bar, the status bar (player
  zone, busy pill, counts).
- **Side panels**: TAGGER (ONLINE with its release cards, EDITOR with fields
  and cover, FROM NAME, FROM TEXT), RENAMER, GENERATOR, DEDUPLICATOR with its
  results view, EXPORTER.
- **Windows**: Settings, the mask builder in its three modes, Transform, the
  preset editor, Add column, Confirm.
- **Menus and popovers**: library health, columns, filter presets, mask
  presets, recent folders, grouping, the placeholder reference, the two
  context menus, the player's volume.
- **Feedback**: toasts and run reports, the busy pill, empty states.

## What not to reopen

Recent rounds settled these; report a fault in how they are applied, not the
decisions: the side-panel rule (jobs packed at the top, a closing grid of up
to three cells with the run button last, `docs/design/side-panel-layout.md`);
the input group (`.field`); the window heads and their close button per
platform; the report toast tied to the Apply bar.

## Deliverable

- **A ranked list of findings**: each with where it is (surface and selector or
  file), what is wrong, the condition that shows it (theme, language, width),
  how sure you are (seen in the code, or only in your render, which can differ
  from WKWebView), and the fix as CSS, or as markup where it has to change.
- **Grouped into batches** that can each be folded in as one change: for
  example "top bar and table toolbar", "side panels", "windows", "menus",
  "typography scale". Within a batch, most severe first.
- **A preview card per batch** (light and dark) showing before and after for
  the main findings.
- No new components and no new tokens unless a finding cannot be fixed
  without one; say so where that is the case.
