# Settings

Opened from the sliders icon at the top right.

**Save commits, Cancel discards** — and Escape is Cancel. That applies to
everything on this page except three controls that are deliberately live, because
their whole point is seeing the effect: **Theme**, **Value font** and the two font
**size sliders** change the interface the moment you touch them, and are kept
whether or not you press Save.

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

**Carry sidecar files** — when a rename or move relocates a track, same-named
files move with it, and undo restores them together. A file already sitting at
the destination is never overwritten.

**Extensions** — which sidecars count. Space- or comma-separated, without the
dot, case-insensitive. The defaults cover lyrics, cue sheets and per-track cover
images.

## Display

**Theme** — Auto follows your system appearance; Light and Dark force one.

**Selection checkbox column** — adds a checkbox column to the file table, and a
select-all checkbox in its header. Off by default, since rows select on click.
Unlike the two controls above it, this one takes effect on **Save**.

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

A few purely visual preferences — theme, column layout and widths, filter flags,
grouping key, volume, the LAB fonts — are stored by the interface itself rather
than in `settings.json`.

**Nothing is stored inside your music folders**, and no database of your
collection is kept. Delete the directory above and TagRex forgets your settings
and your undo history; your music is untouched.
