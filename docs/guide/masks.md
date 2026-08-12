# Mask reference

A mask is a pattern that turns tags into a string: a file name, a folder path, a
line in a report. `%artist% - %title%` is a mask.

The same grammar is used by **Rename by mask** and **Reorganize into folders** in
[RENAMER](renamer.md), by **Tags from the file name** in [TAGGER](tagger.md) —
which reads it the other way — by the **Report** format in
[EXPORTER](duplicates-and-export.md), and by a mask-defined
[table column](file-table.md).

## Placeholders

Write a field name between percent signs. Names are case-insensitive.

| Placeholder | Field |
| --- | --- |
| `%artist%` | Artist |
| `%title%` | Title |
| `%album%` | Album |
| `%albumartist%` | Album Artist |
| `%track%` | Track number |
| `%tracktotal%` | Total tracks |
| `%disc%` | Disc number |
| `%disctotal%` | Total discs |
| `%year%` | Year |
| `%genre%` | Genre |
| `%comment%` | Comment |
| `%composer%` | Composer |
| `%publisher%` | Publisher / label |
| `%bpm%` | BPM |
| `%isrc%` | ISRC |
| `%key%` | Initial key |
| `%catalognumber%` | Catalogue number |
| `%url%` | URL |
| `%media%` | Media type |
| `%side%` | Vinyl/cassette side letter — **name only**, see below |
| `%skip%` | Matches and discards a run of text — **read only**, see below |

**The same list is in the app**, behind the **?** button beside every pattern
box: every placeholder, grouped, with a line of description, and a click inserts
one at the caret. It works offline, which is when a reference is most needed.

That in-app list is the authoritative one — it is generated from the parser's own
tables and tested against them, so what it shows is what parses. The tables on
this page are written by hand for reading; if the two ever disagree, the app is
right.

An unknown name is an error rather than being passed through as literal text, so
a typo tells you instead of quietly producing `%artsit% - Title`. Hovering a
table column header names the placeholder that addresses it.

Custom (non-modeled) fields cannot be addressed from a mask yet.

## Zero-padding

`%field:width%` pads a numeric value with leading zeros to at least `width`
digits. Non-numeric values are left alone.

```
%track%        →  07
%track:3%      →  007
%disc:2%       →  01
```

**Track numbers pad to two digits by default**, everything else prints as-is.
That default exists because of one specific failure: `%disc%%track%` would
otherwise render disc 1, track 1 as `11`, which a player reads as track eleven.
With the default it is `101`. Set an explicit width when a release needs
something else.

## Optional sections

Square brackets mark a section that is kept only if a placeholder inside it
resolved to something, and dropped **whole** otherwise — including its literal
text.

```
%album%[ (%year%)]
```

- Album with a year → `Blue Lines (1991)`
- Album without one → `Blue Lines`, with no trailing space and no empty
  parentheses

This is what lets one mask serve a library where some releases have a year and
some don't. Without it you would need two masks and a way to decide between them.

Sections nest.

## Literals

The characters `%`, `[` and `]` are reserved. To use one literally, wrap it in
single quotes:

```
'100%'         →  100%
'['live']'     →  [live]
```

Two quotes in a row (`''`) produce one literal quote.

## Folder paths

In **Reorganize into folders**, `/` (or `\`) separates directories:

```
%albumartist%/%year% - %album%/%track% - %title%
```

Paths are built under the opened library root. A separator in a *tag value*
cannot create a folder — only separators you write in the pattern do. An artist
literally named `AC/DC` becomes one folder, not two.

## `%side%` — vinyl sides

`%side%` renders the side letter derived from the disc number: disc 1 → `A`,
disc 2 → `B`, and so on. It is blank for non-vinyl media.

```
%side%%track:1% - %title%    →  A1 - Safe From Harm
```

It is **name only** (what the in-app reference calls it): computed from the disc
number rather than stored, so a mask containing it can build a name but cannot be
used to read tags back out of one.

## File and technical placeholders

Not everything worth putting in a name or a report is a tag. These describe the
**file** instead.

| Placeholder | Value |
| --- | --- |
| `%filename%` | Name without the extension |
| `%fileext%` | Extension alone, no dot |
| `%filenameext%` | Name and extension |
| `%filepath%` | Full path |
| `%foldername%` | Containing folder |
| `%foldername2%` | Its parent |
| `%foldername3%` | And its parent |
| `%_length%` | Duration as `m:ss` (`h:mm:ss` past an hour) |
| `%_length_sec%` | Duration in whole seconds |
| `%_bitrate%` | Audio bitrate, kbps |
| `%_samplerate%` | Sample rate, Hz |
| `%_channels%` | Channel count |
| `%_codec%` | Container name — `MP3`, `FLAC`, `APE` |
| `%_filesize%` | Size, human-readable — `7.3 MB` |
| `%_filesize_bytes%` | Size in bytes |
| `%_filedate%` | Last-modified date, `YYYY-MM-DD` (UTC) |

The leading underscore marks the technical ones: properties of the audio rather
than of the file's place on disk. UTC rather than local time for the date, so the
same file renders the same name on any machine.

All of them are **name only**, for the same reason `%side%` is — there is no tag
to read a bitrate back into, and pulling `%filename%` out of a filename says
nothing. A mask carrying one works in Rename, Reorganize, Report and a mask
column, and is refused by **Tags from the file name**.

A value that isn't available renders as empty rather than failing: an unreadable
file, a folder level above the root, a container that reports no bitrate. Wrap it
in `[...]` if the surrounding text should disappear with it.

`%filepath%` is sanitized like every other rendered value, so its separators are
stripped. It identifies a file; it does not reconstruct a path.

## Two directions

The mask engine is bidirectional by design — one grammar that both *renders* a
name from tags and *extracts* tags from a name — and both directions come from
the same parsed pattern, so they cannot drift apart.

Both are reachable. **Rendering** builds a name from tags: RENAMER's *Rename by
mask* and *Reorganize into folders*, the EXPORTER's report, and a mask-defined
[table column](file-table.md). **Extraction** reads tags out of a name, in
TAGGER › [**Tags from the file name**](tagger.md) — the same placeholders, read
the other way, plus `%skip%` for the junk in a filename that maps to no tag. It
produces an ordinary plan, so the same in-table diff, Apply and undo apply.

A few placeholders work in one direction only, and the reference marks which:

| | Direction | Why |
| --- | --- | --- |
| `%side%` | name only | computed from the disc number, so there is no tag to read it back into |
| File and technical (`%filename%`, `%_bitrate%`, …) | name only | properties of the file, not tags |
| `%skip%` | read only | it discards text, so there is nothing to render |

Two placeholders with nothing between them can be rendered but not extracted —
`%disc%%track%` produces `101`, and nothing says where to split it again. Give
one of them a width (`%disc:1%%track:2%`) and it becomes unambiguous.

## Practical patterns

```
%artist% - %title%
%track% - %title%
%albumartist% - %album%[ (%year%)]/%track% - %title%
%albumartist%/[%year% - ]%album%/%track% - %title%
%side%%track:1% - %artist% - %title%
[%catalognumber% - ]%album%
```

Read the other way, in **Tags from the file name**:

```
%track% - %artist% - %title%
%albumartist%/%album%/%track% - %title%
%disc:1%%track:2%_%artist%_-_%title%
%skip% - %artist% - %title%
```

## When a mask can't render

A mask that needs a tag a file doesn't have cannot render for that file. Rather
than writing a name with a hole in it, that file is left out of the plan — you
will see it missing from the preview. Wrap the fragile part in `[...]` if you
want those files handled anyway.
