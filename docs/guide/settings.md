# Settings

Opened from the sliders icon at the top right.

**Save commits, Cancel discards** — and Escape is Cancel. That applies to
everything on this page except the controls that are deliberately live, because
their whole point is seeing the effect: **Theme**, **Accent colour**, **Value font** and the two font
**size sliders** change the interface the moment you touch them, and are kept
whether or not you press Save. **Shortcuts** apply at once too.

**Search** — the box at the top narrows the page to the settings that match, by
what they are called, in the language the interface is in, across every group.
Several words must all be there; a group whose name matches shows in full.
Escape empties the box (a second Escape closes Settings).

**Reset to defaults** — a group whose settings differ from how TagRex ships shows
a *Reset to defaults* link at its top right, and only then. It covers Network,
Tag defaults, Cover art, Online import, Files, Display and LAB. The first five
go back in the form and are kept when you Save, like any other edit; Display and
LAB are live controls, so theirs applies at once. The Discogs token and the
Beatport account have no default and no reset, and Shortcuts keep their own
*Reset all*.

Everything here is stored on this machine, in the app's own config directory:
none of it travels with your music or syncs anywhere.

**Version and About** — the footer shows which version of TagRex is running.
Clicking it opens the About dialog, as clicking the `/tagrex/` wordmark at the
top left does (on macOS, so does *About TagRex* in the app menu). The dialog
links to the source code, this guide, the releases and what changed in them,
the acknowledgements and the license.

## Discogs

**Personal token** — required for Discogs search, release lookups and cover
fetches. Generate one in your Discogs account settings; it is stored locally in
the app's own config directory and never leaves your machine except in requests
to Discogs.

MusicBrainz needs no token.

## Network

**Proxy** — `http://host:port`, or blank for none.

**Rate limit** — requests per minute; `0` means no limit. This throttles *your*
side. A `429` response with `Retry-After` from the provider is always honoured
regardless of what you set here, so lowering this is about being a good citizen,
not about avoiding errors.

**Check for updates** — on by default. When TagRex starts it makes one request
to GitHub's public Releases API for the latest published release and compares
its version with the one you are running. If a newer one exists, a quiet note
appears in the top bar, and the cross beside it stops the note for that version.
The request carries nothing about you or your files beyond what any HTTPS
request does: your address and a user agent naming the TagRex version. The proxy
above applies to it. Switched off, the app makes no such request. **Check now**
asks once regardless and tells you what it found.

When the release can be installed from inside the app, the note reads **Update
to TagRex X** and the document button beside it opens the release page. Clicking
the note asks first, then downloads the update, checks its signature against the
key built into TagRex, installs it and restarts. Nothing is downloaded until you
ask. macOS replaces the app where it is; Windows runs the installer, which shows
its own progress and, when TagRex sits in a folder you cannot write to such as
`Program Files`, first asks for administrator rights; on Linux the system asks for your password to install the new
`.deb` or `.rpm`. Apply or discard your changes, and let a BPM run finish, before
updating. Where there is nothing to install (a release without update bundles,
or a copy that was not installed from a package), the note opens the release
page instead.

## Tag defaults

**ID3 version** — which ID3v2 revision to write for MP3, AIFF and WAV. v2.4 is
the modern default; v2.3 is worth choosing if you use older software that never
learned to read v2.4.

**Read priority** — when a file carries more than one tag block (ID3v2, Vorbis,
APE), values are read from the highest one present. Drag to reorder; there is a
Reset. Most files carry a single block, so this rarely matters — it exists for
the files where it does.

## Cover art

**Max size** — pixels on the longest side. A larger fetched or chosen cover is
downscaled before embedding, so a 3000px sleeve doesn't get baked into every
track. `0` disables resizing.

**JPEG quality** — 1–100, used when a cover is resized.

## BPM detection

**Detect BPM**: on by default. Off hides the metronome on the table toolbar,
and its shortcut does nothing.

**Skip files longer than**: 20 minutes by default, `0` for no limit. A DJ mix
has no single tempo, and measuring one takes as long as dozens of tracks, so a
file longer than this is passed over, judged by the length its header states
before anything is decoded.

**Detected BPM range**: the range Detect BPM folds a measured tempo into, by
halving or doubling it. **As measured** (the default) writes the tempo as the
beats fall. Choose **70-140** if your library writes a 174 BPM track as 87;
**90-180** keeps drum & bass at full tempo and doubles a slow one.

## Online import

**Fields**: which tags an import from Discogs or MusicBrainz may write, one
checkbox each, all on by default. A field switched off is left exactly as the
file has it; see [Import, then fix](tagger.md#import-then-fix).

**Cover art**: whether an import also embeds the release's cover: **Never**,
**If missing** (the default, only files with no artwork at all) or **Always**.

**Vinyl numbering**: how an import numbers a release whose tracks sit on sides
(`A1`, `B2`). The tag standards define the track number as a number, so a side
letter has to go somewhere else.

| Choice | Disc | Track number |
|---|---|---|
| **By record** (the default) | each record: sides A and B are disc 1, C and D disc 2 | runs on across both sides: `A1`, `A2`, `B1` are 1, 2, 3 |
| **Position as is** | from the release, as for any other medium | `A1` as it is (an MP4 file still gets the number, it stores an integer) |
| **Side = disc** | each side: A is disc 1, B disc 2 | per side: `B2` is 2 |

By record numbers from the whole tracklist, so leaving a track out of the import
does not renumber the others, and the disc total counts the records (the sides
under Side = disc). A single-sided record (a set stating as many records as it
has sides) is a disc of its own, and in a set with other media (vinyl plus a CD)
the other media come after the records. Whichever you choose, the position
itself is kept in the `POSITION` field, which the `%position%` and `%side%`
placeholders read. A position that is not a vinyl side, such as `1-05`, still
gives a number. Cassette sides are numbered the same way.

## Files

**Same-named file extensions** — which same-named files travel with a renamed
or moved track. Space- or comma-separated, without the dot, case-insensitive.
The defaults cover lyrics, cue sheets and per-track cover images. Whether they
travel at all, and whether the rest of a folder goes with its album, are
checkboxes on the [RENAMER](renamer.md#sidecar-files) panel.

**Safe file names**: on by default. Names a rename or a transform creates also
work on Windows and on USB sticks and SD cards: `:` becomes ` -`, `?` and `*`
are left out, `" < > |` become `_`, and a trailing dot or space is dropped. Off
keeps these characters where your system allows them; on Windows they are
always replaced. See [RENAMER](renamer.md#reorganize-into-folders).

## Display

**Theme** — Auto follows your system appearance; Light and Dark force one.

**Accent colour** — the colour of buttons, selections, links and the focus ring.
Pick one of the nine swatches (the first is the brand green, the default) or use
the rainbow swatch for any colour you like; **Reset** returns to the brand green.
Text on the accent is kept readable: a colour that would be too light for white
labels is darkened a little, and as text it is lightened or darkened for the
current theme. The green of confirmed states and the red of errors do not change.

**Selection checkbox column** — adds a checkbox column to the file table, and a
select-all checkbox in its header. Off by default, since rows select on click.
Unlike the two controls above it, this one takes effect on **Save**.

## AI assistants

Lets an AI assistant read the library that is open in TagRex, over the Model
Context Protocol (MCP). Supported: **Claude Code** and **Claude Desktop**.

An assistant can list tracks and their tags, read a file's tag blocks, see
what a mask renders to, list the mask placeholders and look for duplicates. It
only sees files the open library lists. It works while TagRex is running; with
no library open, it is told to ask you to open one.

It can also look releases up through TagRex's online sources (Discogs,
MusicBrainz, Beatport, Bandcamp, Soundeo) and read a release's tracklist. The
search uses the Discogs token and Beatport sign-in saved in TagRex, and the
proxy and rate limit set here; neither the token nor the sign-in is ever handed
to the assistant. And it can read the settings that decide what TagRex writes,
such as the ID3v2 version, how an import numbers vinyl and fills the genre, the
multi-value separator and your saved mask presets, so its proposals match your
imports. Connection settings such as the proxy are not shared.

**It never writes a file itself.** When you ask it to change tags, or to rename
or move files, it proposes the changes: they appear in the file table as an ordinary preview, with a chip
above the Apply bar naming the assistant and what it proposes, and TagRex asks
for your attention. Review them like any other edit, untick files or change a
value, then click **Apply** or **Discard**. Applied changes go into the history
marked with the assistant's name and can be undone like any other batch. The
assistant can ask whether you applied or discarded its proposal, and it cannot
delete anything.

A rename is built by the RENAMER from a mask, exactly as if you had typed it
there: either in place, or reorganized into folders from the library root, with
the folders it empties removed. An assistant cannot move files outside the open
library, and cannot rename the library folder itself: open its parent folder
first.

A proposal is refused while the table already shows unapplied changes, your own
or an earlier proposal, so it never mixes into work you have staged. Values a
field cannot hold, locked fields and files outside the library are left out,
and the assistant is told which. A proposed field the table has no column for
shows in a temporary column while the proposal is staged.

Click **Claude Code** or **Claude Desktop** to make a connection. Its card shows
what to do next:

- **Claude Code**: a `claude mcp add` command. Run it in a terminal, then start
  a new Claude Code session.
- **Claude Desktop**: a snippet for `claude_desktop_config.json` (in Claude
  Desktop: Settings, Developer, Edit Config). Merge it into the file and restart
  Claude Desktop. TagRex never edits that file itself.

The command names TagRex itself, started with `--mcp-stdio`, and a credential
file. Each connection has its own random key in that file; TagRex keeps only a
hash of it. **Copy** shows the same setup again at any time. **Remove** deletes
the connection, and the assistant is refused from its next request on.

## Shortcuts

Every keyboard shortcut in the app, one row per action. Click a combination and
press the new one: it takes effect and is stored at once, like the theme, so
Cancel does not take it back. **Escape** while recording gives up and leaves
the old one. **Reset** next to a changed row brings its default back, and
**Reset all** restores every default.

A combination can belong to one action only: picking one that is taken is
refused, and the message names the action holding it. The ones the system or
text editing needs — ⌘C, ⌘V, ⌘X, ⌘Q, ⌘W, Tab, the arrows and Space on their own,
and a few more — cannot be taken at all.

Shortcuts follow the physical key, not the letter it types, so they work the
same with a Cyrillic or any other keyboard layout. The tooltip of a button that
has a shortcut shows it after its own text.

| Action | macOS | Windows and Linux |
| --- | --- | --- |
| Apply the staged changes | ⌘↩ | Ctrl+Enter |
| Discard the staged changes | ⌘⌫ | Ctrl+Backspace |
| Undo the last applied batch | ⌘Z | Ctrl+Z |
| Clear the selected files' tags | ⇧⌘X | Ctrl+Shift+X |
| Detect the selected files' BPM | ⇧⌘B | Ctrl+Shift+B |
| Move the selected files to the Trash | ⌦ (fn+⌫) | Del |
| Open a folder | ⌘O | Ctrl+O |
| Re-read the open folder | ⌘R | Ctrl+R |
| Go to the filter | ⌘F | Ctrl+F |
| Select every row | ⌘A | Ctrl+A |
| Play / pause | ⌥Space | Ctrl+Shift+Space |
| Previous / next track | ⌘← / ⌘→ | Ctrl+Left / Ctrl+Right |
| Switch to TAGGER … EXPORTER | ⌘1 … ⌘5 | Ctrl+1 … Ctrl+5 |
| Show or hide the side panel | ⌥⌘S | Ctrl+Alt+S |
| Open Settings | ⌘, | Ctrl+, |

While you type in a text field or edit a cell, the field keeps the keys it
uses itself — ⌘A selects its text, ⌘Z undoes typing, ⌘⌫ and ⌘← move or delete
within the line. Opening a folder, re-reading it, the filter, the mode switches
and Settings work from anywhere. No shortcut acts while Settings or a dialog is
open, and one whose button is unavailable — Apply with nothing staged — does
nothing.

## LAB

Typography still being evaluated. These may change or be dropped in a later
release, which is why they are grouped apart from the settled Display options
rather than mixed in among them.

**Value font** — the face used for file names, tag values, tracklists and pattern
fields. **Mono** keeps columns aligned and `0`/`O` distinct; **Sans** matches the
rest of the interface; **Condensed** fits more text before a value truncates.

**Table font size** and **Tracklist font size** — sliders, with a live preview as
you drag.

## Where things are stored

TagRex keeps its own data in the standard per-user application directory for your
platform, under `com.tagrex.desktop`:

| Platform | Location |
| --- | --- |
| macOS | `~/Library/Application Support/com.tagrex.desktop/` |
| Linux | `~/.config/com.tagrex.desktop/` |
| Windows | `%APPDATA%\com.tagrex.desktop\` |

| File | What it is |
| --- | --- |
| `settings.json` | Everything on this page that lives in the backend |
| `journal.sqlite` | The undo journal — every applied batch, across libraries |
| `discogs_token` | Your saved Discogs token |
| `mcp/` | AI assistant connections: their keys (hashed), the credential files and where the running window listens |

A few purely visual preferences — theme, accent colour, column layout and widths, filter flags,
grouping key, volume, keyboard shortcuts, the LAB fonts — are stored by the
interface itself rather than in `settings.json`.

**Nothing is stored inside your music folders**, and no database of your
collection is kept. Delete the directory above and TagRex forgets your settings
and your undo history; your music is untouched.
