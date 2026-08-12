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

## Sidecar files

When a rename or move relocates a track, same-named files travel with it: `.lrc`
lyrics, `.cue` sheets, per-track cover images. They are part of the same batch,
so undo restores them together with the track.

This is on by default and configurable — which extensions count is in
[Settings › Files](settings.md#files). A file already sitting at the destination
is never overwritten.

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
