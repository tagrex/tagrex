# Design brief: the transform windows, panel hints, long operations, library health and the small dialogs

> For Claude Design. The repo is linked locally, so **read the current code
> directly** rather than trusting this summary: `app/ui/index.html`,
> `app/ui/style.css` and the modules named in each part. Your earlier rounds are
> the reference for the house style: the mask builder (`foundations/mask-builder.*`)
> and the input group (`foundations/input-group.*`). This brief is **intent
> only**; the layouts and component choices are yours.

Everything below was built in code, one feature at a time, without a design
pass. Five parts, in priority order. Parts 1 and 2 belong together: setting up
transforms and writing masks should read as one system of windows, with only
what is needed left in the panels.

## Part 1: the preset editor and the Transform window (priority)

- **Preset editor**: `#pe-editor`, built by `js/presets.js`. Presets on the left
  (yours, then the built-ins), the open preset's rules on the right as cards, a
  "Try it on" sample under them. A built-in preset is read-only and its whole
  rule area shows the disabled stripes, which reads heavy: every field striped,
  placeholder text ghosted inside the stripes.
- **Transform window**: `#transform-modal`, which borrows the GENERATOR panel's
  `#transform-block` (`js/generator.js`, `js/chains.js`): which presets run for
  the current job, ticked in a checklist, plus "Edit presets…". It is opened
  from the table toolbar's wand and now from inside the mask builder, stacked
  over it.

Open questions:

1. How a **read-only built-in** reads without striping every control: it is
   reference, not a broken form.
2. The **rule card**: kind, target field, find / replace inputs and flags, the
   reorder and remove actions. Use the input group where it fits.
3. How the **checklist** says which presets apply to this job (the ones that
   don't are dimmed today) and how it leads into editing.
4. **Stacking**: the Transform window over the mask builder, the preset editor
   over both. Sizes and how the layers read.

## Part 2: the hints in the panels

RENAMER, FROM NAME and FROM TEXT each carry a long paragraph under the mask
(`.hint` blocks in `index.html`) explaining the grammar: placeholders, optional
sections, widths, folders. On a normal window it takes half the panel. Most of
it is now in the placeholder reference that the mask builder keeps open, and in
the user guide.

Open question: **what stays in the panel**: the one-file example, a line that
points to the builder and the reference, nothing at all? Where does the rest
live, and how is it found by someone who has never opened the builder?

## Part 3: long operations, progress and outcome

- BPM detection (`js/bpm.js`): the toolbar button becomes Stop while it runs,
  progress is a small bar and a count in the status bar's busy pill
  (`#busy-pill`), and the outcome is a toast of several clauses, e.g. "Stopped
  after 6 of 160 · Previewing BPM on 4 files · 1 long file skipped · no steady
  beat in 1 file".
- Apply and undo (`js/applyprogress.js`) report progress in the diff bar and the
  same busy pill.
- Toasts (`#toast`, `toast()` in `js/dom.js`) are single lines, error or not.

Open questions: one pattern for **progress, stopping, and the summary of what
happened**, so a long operation reads the same wherever it runs. Whether a
multi-part outcome stays a toast, and how its parts read (counts that are
problems versus counts that are fine).

## Part 4: the Library health menu

`#health-menu`, built by `js/health.js` from `js/healthchecks.js`: groups of
checks (missing tags, suspect values, albums, library, files) with a count each;
picking one filters the table. Some checks now carry details that unfold in the
menu: albums with tracks missing ("CD 2: no 3, 7-8"), values spelled several ways
across the library.

Open question: the menu has outgrown a plain list. Its layout, how details
unfold, and how an active check shows on the button and in the table.

## Part 5: the small dialogs

Each too small for a round of its own:

- **Add column** (`#colmask-modal`): name, mask pattern (already an input
  group), alignment.
- **Confirm** (`#confirm-modal`): yes/no prompts such as overwriting covers.
- **Context menus**: `#file-ctx` (remove from list, move to Trash) and
  `#edit-ctx` (cut, copy, paste, select all).

Open question: bring them in line with the rest, consistent with the modal and
menu shells used elsewhere.

## Deliverable

A preview card per part (light and dark), plus the CSS to fold back into
`app/ui/style.css` and, where the markup has to change, the diff as in the
earlier rounds. Prefer extending existing classes over adding new ones.
