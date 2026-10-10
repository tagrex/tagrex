# Settings

Opened from the sliders icon at the top right.

**Save commits, Cancel discards** — and Escape is Cancel. That applies to
everything on this page except the controls that are deliberately live, because
their whole point is seeing the effect: **Theme**, **Accent colour**, **Value font** and the two font
**size sliders** change the interface the moment you touch them, and are kept
whether or not you press Save. **Shortcuts** apply at once too.

The footer reads *Saved to this machine* as a reminder that none of this travels
with your music or syncs anywhere.

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

## Files

**Same-named file extensions** — which same-named files travel with a renamed
or moved track. Space- or comma-separated, without the dot, case-insensitive.
The defaults cover lyrics, cue sheets and per-track cover images. Whether they
travel at all, and whether the rest of a folder goes with its album, are
checkboxes on the [RENAMER](renamer.md#sidecar-files) panel.

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

A few purely visual preferences — theme, accent colour, column layout and widths, filter flags,
grouping key, volume, keyboard shortcuts, the LAB fonts — are stored by the
interface itself rather than in `settings.json`.

**Nothing is stored inside your music folders**, and no database of your
collection is kept. Delete the directory above and TagRex forgets your settings
and your undo history; your music is untouched.
