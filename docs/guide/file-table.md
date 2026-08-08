# The file table

Every mode acts on the table, so how well you can carve a selection out of it
decides how quickly anything else goes. This page is about shaping the view and
picking rows; what the modes then *do* with them is on their own pages.

## Selecting rows

| Gesture | Result |
| --- | --- |
| Click a row | Select just that row |
| ⌘/Ctrl-click | Add or remove one row, keeping the rest |
| Shift-click | Select the range from the last-clicked row |
| ↑ / ↓ | Move to the previous/next row |
| Space | Toggle the focused row in or out of the selection |

To select everything, click the first row and Shift-click the last. There is no
⌘/Ctrl-A: turn on the checkbox column below if you want a select-all control.

When a library is opened, only the first file is selected — not the whole
library, so an operation aimed at "the selection" can't run away with the lot
before you have looked at anything.

The selection count is always in the status bar at the bottom, and every panel
that acts on it repeats the count in its own heading.

**A checkbox column** can be turned on in **Settings › Display** if you prefer
ticking to clicking. It is off by default, and it brings a select-all checkbox in
the table header with it.

### Selecting a whole folder

With grouping on, clicking a **group header** selects everything in that group
and expands it so you can see what you got. The modifiers mirror row clicks:
⌘/Ctrl-click toggles that group in or out of the selection without disturbing
other groups, and Shift-click extends a range from the current anchor through
the whole group.

## Sorting

Click a column header to sort by it; click again to reverse. Sorting is a view,
not an operation — it changes the order rows are listed in, never the files.

That order does matter for one thing: operations that walk the selection "in
table order", such as track numbering in [GENERATOR](generator.md), take the
order you are looking at.

## Grouping

The group button in the toolbar groups rows by any modeled field. The menu
promotes the keys actually reached for — **None**, **Folder**, **Release id**,
**Artist**, **Album**, **Album Artist** — above a separator, with the remaining
fields below.

Grouping is also a view. The button tints while it is on, and a second button
appears to collapse or expand every group at once.

Folder group headers show the path from the session root rather than just the
last component, so two folders both named `CD1` stay distinguishable.

## Filtering

The filter box narrows the visible rows as you type. Three things it does beyond
plain substring matching:

- **Field scope.** `artist:aphex` searches only the Artist column. Any known
  column name works as a prefix.
- **Regular expressions.** The `.*` toggle inside the box switches from
  substring to regex matching. A pattern that doesn't compile turns the box red
  rather than silently matching nothing.
- **Case sensitivity.** The `Aa` toggle. Off by default.

The filter sees the values the table shows, sorted the way the table is sorted,
so a field-scoped query finds what your eye finds.

> Filtering hides rows; it does not deselect them. A selected row that scrolls
> out of view because of a filter is still selected, and still gets operated on.
> Clear the filter if you want to be certain what you are about to change — the
> preview will show you regardless.

## Columns

**Columns** in the toolbar opens a list of every available tag field. Tick the
ones to show, drag the grips to reorder them, and drag a column's right edge in
the table to resize it. Hidden fields collect under a separator at the bottom of
the menu. A footer resets the whole thing — set, order, visibility and widths —
to the defaults.

Column layout is remembered between sessions.

## Presets

**Presets** saves the current filter *and* sort under a name, and re-applies both
in one click. Useful for the queries you re-run constantly: everything missing a
year, everything from one label, everything whose title still has underscores in
it.

These are view presets. The saved chains of *transform rules* are a different
thing, and live in [GENERATOR](generator.md).

## Editing cells in place

**Double-click** a cell to edit it — a single click selects the row instead, so
the two never fight. Enter commits the cell and leaves edit mode; clicking away
commits it too.

There is no per-cell undo. To back out of an edit, either retype the old value,
or press **Discard** on the preview once you have staged it — that throws away
every pending edit at once.

Edited cells are marked as dirty and nothing is written yet: the edits pile up
until you press **Preview edits** in the status bar, which turns them into a
change plan like any other operation. See
[Change plans, Apply and Undo](change-plans.md).

## Clearing tags

The eraser at the end of the toolbar wipes the text tags of the selected files.
It is separated from the rest of the toolbar by a divider because everything to
its left configures the view, while this one acts on your files.

What it removes: modeled text fields. What it keeps: cover art and DJ cue
points. Like everything else, it stages a preview first, so Apply/Discard still
gates it, and it is undoable afterwards.

## The player

The player is for confirming that a file is what its tags claim, not for
listening to a collection.

It appears when a track starts and leaves when playback stops; while it is down,
the play control lives in the status bar so it is always reachable. Playback is
gapless and auto-advances through the table's visible order, stepping over group
headers and collapsed rows.

Controls: previous, play/pause, next, a seek slider, a three-state **repeat**
(off, repeat all, repeat this track — shown by the icon tinting and a small "1")
and **volume** with a mute toggle that remembers the level to come back to. The
volume level persists between runs.

The track name comes from the tags — "Wish Mountain — Radio" rather than
`102_wish_mountain_-_radio.mp3` — falling back to the file name when the tags
are empty.
