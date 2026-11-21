# RENAMER

Two operations that both take a [mask](masks.md): rename files in place, and
move them into a folder structure. Both stage a preview before anything moves.

## Rename by mask

Type a pattern, press **Preview rename**, read the diff, Apply.

```
%artist% - %title%
```

The extension is never part of the mask — it is carried over from the original
file. To change an extension, use the `File extension` scope in
[GENERATOR](generator.md).

Files whose tags can't fill the mask are left out of the plan rather than renamed
to something with a hole in it. If half your selection is missing from the
preview, that is usually the reason: wrap the fragile part in `[...]` so those
files still get a name.

### The mask builder

For a longer expression, the button at the end of the mask row opens the mask
builder: the mask in a field of its own that grows with it as it wraps, the
placeholder and function reference kept open beside it with a filter, and
below, every selected file as its current name and the name the mask gives it,
recomputed as you type. A click in the reference inserts at the caret, and a
function arrives with the caret inside its parentheses. The field's own buttons
sit at the end of its first line: × empties it to start over (⌘Z brings the
mask back), the bookmark opens the mask presets, and the wand opens Transform.

The ticked **Transform** presets run on every new name, so the builder keeps
them in reach: the wand opens the same Transform window as the table toolbar,
over the builder, and shows the number of rules that run. Ticking or editing a
preset there recomputes the list at once, and the line above the list says how
many rules ran on the names.

The list is built by the same call as Preview, with the same rules from
**Transform**, and it says what Preview would: a row is marked when the name
stays as it is, when a placeholder the mask requires is empty for that file, and
when two files would end up with the same name, the one case shaded red. A
mistake in the expression, an unknown placeholder or a missing `)`, outlines
the field in red and shows as a line under it while the list keeps the last
result. A large selection lists its first 300 files and
says how many there are.

The window opens nearly as large as the app's and can be resized from its
lower right corner; it opens at that size next time. Each file is one line, the
current name cut to a column (the full path is on hover) and the new name
whole: a name wider than the list scrolls it sideways rather than wrapping.

**Use this mask** (or Enter) puts the mask in the box and stages the preview;
**Cancel** (or Esc) leaves the box as it was. Nothing is written from the
builder. It follows **Reorganize** when that is on, so the new names include the
folders.

## Reorganize into folders

The same grammar, plus `/` (or `\`) to separate directories:

```
%albumartist%/%year% - %album%/%track% - %title%
```

Everything is built under the library root you opened. Directories are created
as needed, and directories left empty by the move are cleaned up afterwards —
and undo removes exactly the directories the batch created, so rolling back a
reorganize doesn't leave a skeleton of empty folders behind.

A tag value cannot introduce a folder. Only the separators you write in the
pattern do, so an artist named `AC/DC` stays one folder.

Every folder and file name a rename or a transform creates is made safe to
create. A separator or a control character becomes `_`, a name that is only
`..` is refused, and a name longer than 255 bytes is shortened, keeping its
extension. With **Safe file names** on (the default, see
[Settings](settings.md#files)), names also work on Windows and on the FAT32
and exFAT of USB sticks and SD cards: `:` becomes ` -` (so `AC: DC` reads
`AC - DC`), `?` and `*` are left out, `" < > |` become `_`, a trailing dot or
space is dropped, and a reserved device name such as `CON` gets a `_` after it.

## Sidecar files

When a rename or move relocates a track, same-named files travel with it: `.lrc`
lyrics, `.cue` sheets, per-track cover images. They are part of the same batch,
so undo restores them together with the track.

It is on by default and switched with **carry same-named files** on the RENAMER
panel, right above the reorganize toggle; which extensions count is in
[Settings › Files](settings.md#files). A file already sitting at the destination
is never overwritten.

## The rest of the folder

When a move or copy takes **every** track out of a folder, and they all land in
the same destination, whatever else is in that folder goes with them — loose
artwork, a rip log, a `Scans` subfolder — so nothing is stranded and **tidy up
empty folders** can remove the folder. The destination's header in the preview
says how many files come along and names them. A folder that still holds other
music, or tracks that scatter to several destinations, is left alone.

This is **carry the rest of the folder**, among the reorganize options. The
operating system's own bookkeeping files (`.DS_Store`, `._*`, `Thumbs.db`,
`desktop.ini`) are never carried, and don't keep an emptied folder from being
tidied away.

## Rename the open folder

When the folder you opened is one release, the last step is often to rename the
folder itself, say to `Various - La Bush (AS 5606) (1996)`. **Rename the open
folder**, below the rename mask, does that from a mask of its own, such as
`%albumartist% - %album% (%catalognumber%) (%year%)`. The line under the mask
shows the name it gives, or why it cannot give one.

The name comes from the tags every file in the folder shares, so they have to
agree: a mask that gives different names for different files (a per-track
title, two albums in one folder) is refused and both names are shown, as is a
placeholder left empty in a file, a name already taken next to the folder, and
the name the folder already has.

**Preview folder rename** shows every file under the new folder name. Apply
renames the folder in one step, so everything in it goes along, files TagRex
does not list included, and TagRex reopens it under the new name, in the path
field and among the recent folders. It applies as a whole, whatever rows are
ticked. Undo puts the old name back and reopens the folder there.

A drop of files has no one folder to rename, so the action is refused there:
open the folder itself.

## Order of operations

Renaming by tags and fixing tags are the same job approached from two ends, and
the order matters:

1. **Get the tags right first** — by hand in [TAGGER](tagger.md), from an online
   source, or with a transform in [GENERATOR](generator.md).
2. **Then rename from them.**

Renaming first and tagging second means doing the work twice, because the names
you just produced were built from the tags you were about to change.

## A worked example

A folder of files named `01.mp3`, `02.mp3`, … with good tags:

1. Open the folder.
2. Select everything: click the first row, Shift-click the last.
3. RENAMER → `%track% - %artist% - %title%` → **Preview rename**.
4. Check that track numbers came out `01`, not `1` — they pad to two digits by
   default.
5. Apply.

The reverse case — good names, empty tags — is **Tags from the file name** in
[TAGGER](tagger.md): the same mask, read the other way. See
[Two directions](masks.md#two-directions).
