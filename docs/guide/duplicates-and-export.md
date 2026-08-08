# DEDUPLICATOR and EXPORTER

Two read-only modes. Neither ever modifies an audio file, which is why neither
goes through the preview gate.

## DEDUPLICATOR

Scans the whole open library for likely duplicates and groups what it finds. It
reports; it does not delete. Deciding which copy to keep is a judgement call, and
the results give you what you need to make it.

Pick a criterion and press **Scan library**:

| By | Two files are duplicates when… |
| --- | --- |
| Artist + Title | Those two tags match |
| Album + Track | They claim the same position on the same album |
| Duration | They are the same length |
| File size | The files are the same size in bytes |
| Identical bytes | The file contents are byte-for-byte identical |

They form a rough ladder from loosest to strictest. **Artist + Title** finds the
same song ripped twice at different bitrates — the case worth finding, and the
one with the most false positives, since a live version and a studio version
share both tags. **Identical bytes** has no false positives at all but only finds
literal copies: the same file downloaded twice, or a backup folder that got
merged back in.

The results take over the main area, grouped, with a summary of what was found.
Nothing here changes anything — act on it yourself, in the file table.

## EXPORTER

Writes a file describing the selected tracks into the opened library folder. Your
audio files are not touched.

| Format | Output |
| --- | --- |
| Playlist | An `.m3u` playlist of the selected tracks, in table order |
| CSV | One row per track with the tag columns — opens in any spreadsheet |
| HTML | A self-contained HTML table — opens in any browser |
| XML | One element per tag, for scripts and other tools |
| Report | Each track rendered through a [mask](masks.md), one line apiece |

Set the **File name** and press **Export**.

Playlist entries are written relative to the playlist when the track sits inside
the library, and absolute otherwise — so a playlist exported next to its music
stays portable if you move the folder.

**Report** is the flexible one: any mask, one line per track. `%artist% -
%title%` for a tracklist to paste somewhere; `%catalognumber% — %album%` for a
label index; whatever the situation needs.

Exports write immediately and are not recorded in the undo journal. There is
nothing to roll back — delete the file if you don't want it.
