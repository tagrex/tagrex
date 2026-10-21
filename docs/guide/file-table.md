# The file table

Every mode acts on the table, so how well you can carve a selection out of it
decides how quickly anything else goes. This page is about shaping the view and
picking rows; what the modes then *do* with them is on their own pages.

## Selecting rows

| Gesture | Result |
| --- | --- |
| Click a row | Select just that row |
| ⌘/Ctrl-click | Add or remove one row, keeping the rest |
| Shift-click | Select the range from the last-clicked row, keeping what a ⌘/Ctrl-click left selected |
| ⌘/Ctrl-A | Select every row the table lists |
| Click below the rows | Clear the selection |
| ↑ / ↓ | Move to the previous/next row |
| Space | Toggle the focused row in or out of the selection |

Two blocks with a gap between them take four clicks: click the first row of
the first block and Shift-click its last, then ⌘/Ctrl-click the first row of the
second block and Shift-click its last. The first block stays — a Shift-range is
added to what the last ⌘/Ctrl-click left, the way a file manager does it.

⌘/Ctrl-A selects every row the table lists — inside a text field it selects the
field's text instead, as usual. While a staged plan is under review it ticks
every changed file for Apply rather than selecting rows. The combination can be
changed in **Settings › Shortcuts**.

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

## Library health

The heart button next to the presets button lists ready-made checks, each with
the number of files it flags. Choosing one narrows the table to those files; the
button lights up, the status line names the check, and choosing it again (or
"Show all files") brings everything back. A check is a view, not an edit: fix
what it shows the usual way and the file drops out of the list as it is
re-read.

- **Missing**: no title, artist, album, album artist, track number, year or
  genre.
- **Suspicious**: the title is the file name (with or without a leading track
  number), a year that is not a year (not `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, or
  before 1900 or after next year), a value with stray spaces around it.
- **Albums**: a track number above its total, tracks of one album that
  disagree on the total, the year or the album artist, and two files with the
  same track number. An album is the files in one folder that share an album
  name; a disc is told apart by its disc number.
- **Library**: the same artist, album artist, album or genre written in more
  than one way (case, accents, runs of spaces). The commonest spelling is taken
  to be the intended one and the files with any other are flagged.
- **Files**: tags that can't be read, and files carrying more than one tag
  block.

The checks work on what is already loaded, so there is no cover check: whether
a file has a picture is not read until it is selected.

The health filter works together with the filter box and is dropped when another
folder is opened.

## Columns

**Columns** in the toolbar opens a list of every available tag field. Tick the
ones to show, drag the grips to reorder them, and drag a column's right edge in
the table to resize it. Hidden fields collect under a **Hidden** separator at the
bottom of the menu. A footer resets the whole thing — set, order, visibility and
widths — to the defaults.

**File** is marked *always shown* and its tick is disabled: it is structural
rather than a tag column, and it stays first. Everything else is yours to
arrange.

### A column of your own

**Add column…** at the bottom of the columns menu defines a column whose value is
a mask, rendered per row: `%artist% — %title%`, `%catalognumber% · %year%`,
`%_bitrate% kbps`. Give it a name, a pattern and an alignment; the **?** button
beside the pattern lists every placeholder.

Such a column is read-only — the value is computed, so there is nothing to edit
in the cell — and it sorts and filters on the rendered text. Its definition is
remembered with the rest of the column layout, and the row in the columns menu
carries buttons to edit or remove it.

It renders **what is on disk**. Staged edits appear once applied, not while they
sit in the preview. A pattern the parser refuses is reported when you save it,
not silently blanked.

### Fitting a column to its content

**Double-click a column's right edge** to size it to its widest value. **Fit to
content** in the columns menu does the same for every column at once, and
**Autofit by content** keeps doing it as the table changes — when rows, columns
or the value font change, not merely when the selection does.

A fitted width is an ordinary width: it persists, and it can be dragged
afterwards. Fitting is clamped at both ends, so a column never ends up narrower
than its own header, and one 400-character comment cannot push the table into
horizontal scrolling on its own.

Hovering a column header names the mask placeholder that addresses it.

Column layout is remembered between sessions.

## Presets

**Presets** saves the current filter *and* sort under a name, and re-applies both
in one click. Useful for the queries you re-run constantly: everything missing a
year, everything from one label, everything whose title still has underscores in
it.

These are view presets. *Transform* presets — named chains of cleanup rules — are a different
thing, and live in [GENERATOR](generator.md).

## Editing cells in place

**Double-click** a cell to edit it — a single click selects the row instead, so
the two never fight. Enter commits the cell and leaves edit mode; clicking away
commits it too.

A committed cell is written at once, as a change of its own in the undo journal:
the undo arrow in the top bar, or ⌘Z, takes back the last one. **Escape** leaves
the cell as it was, writing nothing. A value the field cannot hold — a
three-digit year, a letter in a track number — is not written; the cell goes
back to what the file says and a message says why.

The row stays where it is after the write, even when the edited column is the
one the table is sorted by; it moves the next time you sort or filter.

A cell is the one edit without a preview, because what you typed is already on
screen. Edits across a selection go through **TAGGER › EDITOR**, which shows the
plan first like every other operation. See
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

On macOS the player is a system media player: the keyboard's play/pause, next
and previous keys drive it, and so do headphone controls and the Now Playing
widget in Control Center, which shows the track's title, artist, album and
cover and lets you drag its position. No permission is needed. macOS sends
these keys to whichever app is playing, so while another player is the one
playing, they stay with it.

On Linux the player is an MPRIS media player on the session bus, so the
desktop's media widget (the media controls in the notification panel or the
system tray), the media keys and `playerctl` drive it the same way: play/pause,
stop, next, previous and seeking, with the track's title, artist, album, length
and cover on show. Without a session bus the player works as before, just
without these controls.

On Windows the player is in the system media controls: the media keys,
headphone buttons and the media panel above Quick Settings play, pause and skip
it, and the panel shows the track with its cover. Where the Windows version
gives that panel a position bar, it seeks too.
