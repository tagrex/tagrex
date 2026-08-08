# Change plans, Apply and Undo

This is the mechanism the whole app is built around. Every mode proposes changes
the same way, and they all pass through the same gate, so learning it once covers
everything.

## The cycle

1. **You ask for something** — edit cells, run a transform, preview a rename,
   import a release, embed a cover.
2. **TagRex builds a change plan** and draws it into the file table as a diff.
   Nothing has been written.
3. **You review it**, and untick any row you disagree with.
4. **Apply** writes it, as one transactional batch, recorded in the undo journal.
5. **Undo** rolls the whole batch back if you were wrong.

Steps 2–4 look identical no matter which mode produced the plan. A rename, a tag
edit and a cover embed are all just plans.

## Reading the diff

While a plan is staged, the table shows it in place:

- **Changed cells show their new value.** Rows the plan doesn't touch stay as
  they were, so the change stands out against its context rather than against an
  empty screen.
- **Show old values** in the floating bar reveals the struck-through previous
  value under each changed cell. It is off by default because most of the time
  you want to read what you are about to get, not what you already have.
- **The checkbox column is the apply scope, not the selection.** While a diff is
  staged, the column's meaning switches: ticked rows will be applied, unticked
  rows will not. Space toggles the focused row.
- **The count in the bar** is how many files Apply will touch, not how many the
  plan covers — untick rows and it goes down.
- Rows are inert while a diff is under review: you cannot start editing cells in
  the middle of deciding about a plan.

If the plan turns out to be empty — the rules changed nothing on this selection —
the table simply stays as it was and a message says so. That is a real answer,
not a failure.

## Apply

**Apply** writes the ticked rows. It goes through a transactional executor and is
recorded in the journal as a single batch, so it undoes as a unit: a
reorganize that renamed four hundred files is one entry, not four hundred.

**Discard** throws the plan away. If the plan came from pending inline cell edits
or a release import, Discard also drops those staged values — the plan owned
them. Any other kind of plan just disappears, leaving what it was built from
untouched.

## Undo

The undo arrow in the top bar rolls back the most recent applied batch for the
current library.

- **It is a stack, not a single step.** Each undo takes the newest remaining
  batch and removes it from the journal, so pressing it repeatedly walks back
  through your session.
- **There is no redo.** An undone batch is gone from the journal. If you undo one
  step too far, you have to make the change again.
- **It survives restarts.** The journal is a SQLite database on disk, so closing
  TagRex and reopening the same library leaves the history intact.
- **It is scoped to the library you have open.** The journal is shared across
  every library you have ever opened, but only batches whose files sit under the
  current root are offered — otherwise undo would promise something it cannot
  deliver for files outside the open folder.
- **Renames and moves undo completely**, including removing directories the move
  had to create, and including sidecar files that travelled with the track.

The journal lives with the rest of the app's data — see
[Settings › Where things are stored](settings.md#where-things-are-stored).

## What the gate does not cover

Two operations write immediately, without staging a plan, because neither
touches your audio files:

- **Export** in [EXPORTER](duplicates-and-export.md) writes a playlist, CSV,
  HTML, XML or report into the library folder.
- **Export** in the cover-art well writes the embedded cover next to each file
  as an image.

Both create new files and modify nothing, which is why they skip the preview.
They are also not in the undo journal — delete the files if you don't want them.

Everything that modifies an audio file or its name goes through the gate.
