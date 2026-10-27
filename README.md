<p align="center">
  <img src="assets/logo.svg" width="120" alt="TagRex logo">
</p>

<p align="center">
  <img src="assets/wordmark-dark.svg#gh-dark-mode-only" width="320" alt="/tagrex/">
  <img src="assets/wordmark-light.svg#gh-light-mode-only" width="320" alt="/tagrex/">
</p>

<p align="center">
  <b>A free tag editor for your music library — Windows, macOS and Linux.</b><br>
  Edit tags and file names in a table, see exactly what will change before
  anything is written, and roll any batch back, even after a restart.<br>
  Fill tags from Discogs, MusicBrainz, Beatport, Bandcamp and Soundeo.
</p>

<p align="center">
  <img src="docs/img/platforms-dark.png#gh-dark-mode-only" width="420" alt="Runs on macOS, Windows and Linux. Built with Rust and Tauri.">
  <img src="docs/img/platforms-light.png#gh-light-mode-only" width="420" alt="Runs on macOS, Windows and Linux. Built with Rust and Tauri.">
</p>

<p align="center">
  <img src="docs/img/cascade-dark.png#gh-dark-mode-only" width="900" alt="TagRex windows overlapped: the Tagger with a release card, track-length matching and the player, and behind it a rename previewed as a diff in the Renamer and a duplicate scan in the Deduplicator">
  <img src="docs/img/cascade-light.png#gh-light-mode-only" width="900" alt="TagRex windows overlapped: the Tagger with a release card, track-length matching and the player, and behind it a rename previewed as a diff in the Renamer and a duplicate scan in the Deduplicator">
</p>

<p align="center">
  <img src="docs/img/demo-dark.gif#gh-dark-mode-only" width="900" alt="A folder of untagged files is opened, a release is found on Discogs and matched, the tags are written, then a rename into a new folder is previewed, applied and undone">
  <img src="docs/img/demo-light.gif#gh-light-mode-only" width="900" alt="A folder of untagged files is opened, a release is found on Discogs and matched, the tags are written, then a rename into a new folder is previewed, applied and undone">
</p>

---

> **Status: 0.27.x.** Usable day to day: table editing, rename masks with a
> function language, tags read back out of file names, text transforms, online
> lookups, BPM detection from the audio, cover art, tag block conversion,
> duplicate detection, exports, a preview player, a transactional undo journal,
> and an interface that speaks
> English, German, Spanish, French, Italian, Ukrainian and Russian. Not 1.0,
> so expect rough edges — bug reports and feedback are welcome. How to use it is in the
> [user guide](docs/guide/README.md); the design is written up in
> [docs/architecture.md](docs/architecture.md); user-visible changes are in
> [CHANGELOG.md](CHANGELOG.md).

<details>
<summary><b>Install</b></summary>

Builds are on the [Releases](https://github.com/tagrex/tagrex/releases) page.

| Platform | Architectures | Package |
| --- | --- | --- |
| macOS | Apple Silicon, Intel | `.dmg` |
| Windows | x86-64, ARM64 | `.exe` installer |
| Linux | x86-64, ARM64 | `.deb`, `.rpm` |

The macOS download is one universal app that runs natively on both Apple Silicon
and Intel Macs.

**First launch on macOS and Windows.** The builds are not signed with a paid
developer certificate (no Apple notarization, no Windows code signing), so both
systems warn the first time. On macOS, open **System Settings › Privacy &
Security** and choose **Open Anyway** next to the TagRex message. On Windows,
in the SmartScreen window click **More info**, then **Run anyway**.

**Linux.** Only `.deb` and `.rpm` packages are published for now — there is no
Flatpak, AUR package or AppImage yet
([#503](https://github.com/tagrex/tagrex/issues/503)). From the folder you
downloaded to:

```sh
sudo apt install ./TagRex_*_amd64.deb     # Debian, Ubuntu (arm64.deb on ARM)
sudo dnf install ./TagRex-*.x86_64.rpm    # Fedora (aarch64.rpm on ARM)
```

</details>

<details open>
<summary><b>What it does</b></summary>

**A folder is the library.** Point TagRex at one and it recurses: type or browse
to the path, drop the folder on the window, or hand it over from the file
manager — **Open With**, the Dock icon, or the command line.

**The table is the subject.** Files load into a spreadsheet-style table with
configurable tag columns, grouping by any modeled field, filtering (substring or
regex, optionally field-scoped as `artist:aphex`) and saved filter/sort presets.
Selection is first-class: click, ⌘/Ctrl, Shift ranges, keyboard. Tag cells edit
in place, offering the values the library already holds as you type, and a cell
is written when you press Enter — a change of its own that undo takes back. A button
beside the folder path re-reads what is open without losing your sort, filter or
pending edits, and right-clicking a file offers Detect BPM and Clear tags for
the selection, or to drop it from the list or move it to the system Trash.

<p align="center">
  <img src="docs/img/tagger-dark.png#gh-dark-mode-only" width="800" alt="TagRex Tagger view: a library grouped by folder in a spreadsheet-style table, with the online lookup panel on the right">
  <img src="docs/img/tagger-light.png#gh-light-mode-only" width="800" alt="TagRex Tagger view: a library grouped by folder in a spreadsheet-style table, with the online lookup panel on the right">
</p>

**Nothing is written until you look at it.** Every operation that changes files
in bulk — rename, move, a tag edit across a selection, transform, import, cover
embed — produces a change plan that is
rendered as a diff *into the table itself*, with a per-row apply scope and an
Apply/Discard bar. Applying goes through a transactional executor with a
persistent SQLite undo journal, so a batch survives an application restart and
can still be rolled back.

<p align="center">
  <img src="docs/img/renamer-diff-dark.png#gh-dark-mode-only" width="800" alt="A rename previewed as a diff inside the table, with an Apply and Discard bar at the bottom">
  <img src="docs/img/renamer-diff-light.png#gh-light-mode-only" width="800" alt="A rename previewed as a diff inside the table, with an Apply and Discard bar at the bottom">
</p>

The same batch in Finder, before and after Apply:

<table align="center">
  <tr>
    <th align="center">Before</th>
    <th align="center">After</th>
  </tr>
  <tr>
    <td align="center" valign="top">
      <img src="docs/img/finder-before-dark.png#gh-dark-mode-only" width="272" alt="Finder: a folder named various la_bush as_5606 1996 holding fifteen files named 00001 la bush 1996.mp3 to 00015 la bush 1996.mp3">
      <img src="docs/img/finder-before-light.png#gh-light-mode-only" width="240" alt="Finder: a folder named various la_bush as_5606 1996 holding fifteen files named 00001 la bush 1996.mp3 to 00015 la bush 1996.mp3">
    </td>
    <td align="center" valign="top">
      <img src="docs/img/finder-after-dark.png#gh-dark-mode-only" width="468" alt="Finder: inside that folder a new folder named various_-_la_bush_-_music_from_the_temple_of_house_(as_5606)_(1996) holds files such as 101_the_x_factor_-_desert_rain.mp3">
      <img src="docs/img/finder-after-light.png#gh-light-mode-only" width="468" alt="Finder: inside that folder a new folder named various_-_la_bush_-_music_from_the_temple_of_house_(as_5606)_(1996) holds files such as 101_the_x_factor_-_desert_rain.mp3">
    </td>
  </tr>
</table>

**Fields can be locked.** A padlock beside a field in the EDITOR panel puts it
out of reach: no import, transform, rename or clear-tags can change it, its
table cells stop opening for editing, and a plan reports what the lock kept out
rather than silently doing less. Locks last for the session — one you set months
ago and forgot would be worse than none.

**Problems come to you.** The heart button next to the presets lists ready-made
checks — missing tags, a title that is only the file name, an odd year, stray
spaces, track numbers above their total or repeated, albums whose tracks
disagree, the same artist or genre spelled several ways, unreadable files — each
with a count; choosing one narrows the table to the files it flags.

**Modes**, each a verb applied to that table:

- **TAGGER** — edit tags by hand, or pull them from Discogs (personal token),
  MusicBrainz, Beatport, Bandcamp or Soundeo — the last two for underground and
  self-released electronic music the databases don't carry. Paged search, release
  cards with cover browser and tracklist,
  content-based candidate matching plus exact ISRC matching, auto-align and
  auto-numbering on import. A **FROM NAME** sub-tab runs a mask the other way
  round — the file's own name read back into tags, with `%skip%` for the junk
  that maps to nothing and a live read-out of what the pattern is pulling out.
  **FROM TEXT** does the same with a pasted or opened tracklist, one line per
  selected file, skipping the headings and side labels the mask doesn't fit.
- **RENAMER** — rename files and sort them into folders from one mask
  (`%artist% - %title%`; a `/` in it makes subfolders), with conditional `[...]`
  sections, zero-padding, `%field:width%` and the function language below. What
  the mask makes of the selected file shows under it as you type, and a mask
  that can't render says which tag is empty. The result can also be moved or
  copied into a different folder; the preview groups files under the folder
  they land in. Folder moves create and clean up directories, and same-named
  sidecar files (`.lrc`, `.cue`, per-track covers…) — and, if you like, the rest
  of the folder — travel with the track.
- **GENERATOR** — text transforms: case conversion, find/replace, remove
  diacritics, transliterate Cyrillic and Greek to Latin, musical ⇄ Camelot key
  notation. Rules are grouped into named presets, made in their own editor,
  which tries them on a sample as you build them; every rule names the field it
  acts on, so one preset can upper-case a catalogue number while title-casing
  the titles.

**Cleanup is part of the job, not a second step.** Tick presets in the dialog
behind the wand and they run as part of importing a release, reading tags out
of a file name and renaming — inside that panel's own button: one press, one
plan, one undo entry. Each job keeps its own ticked set and takes only the
rules aimed at what it produces: tag rules for imports and FROM NAME, file-name
rules for RENAMER. So the underscore RENAMER wants never reaches the tags
FROM NAME reads. The tag editor has none on purpose — a value typed by hand
comes out as typed.
- **DEDUPLICATOR** — read-only scan for likely duplicates by a chosen criterion.
- **EXPORTER** — M3U playlists, CUE sheets, CSV, HTML, XML, and mask-based
  reports. A playlist can come out as one list, or one per folder or album with
  the file names rendered from a mask.

**A mask is a small expression language.** Placeholders are only the start:
`$name(arg,arg)` calls wrap them, arguments are patterns in their own right so
calls nest and may hold placeholders and sections of their own —
`$if2(%albumartist%,%artist%)`, `$swapprefix(%artist%)`,
`[$if(%bpm%,' - '$round($div(%bpm%,2)))]`. Forty-one functions in three groups:
reshaping a value (`lower`, `upper`, `caps`, `left`, `substr`, `replace`,
`getpart`, `stripprefix`, `cutmix`…), asking a question about it (`if`, `if2`,
`equal`, `and`, `greater`, `isnumber`, `in`…) and computing with it (`add`,
`sub`, `div`, `mod`, `min`, `max`, `round`). A value counts as true when it is
not empty — the same rule `[...]` already follows. Positions and lengths are in
characters rather than bytes and clamp instead of failing, so what stops a
rename is a bad pattern, not an awkward title. A mask that calls a function is
render-only: substitution is invertible in both directions and `$upper` is not.
Every mask input carries a `?` that lists the whole library, argument by
argument, and the masks of RENAMER, FROM NAME and FROM TEXT open in a builder
window: the mask in its own field, the reference beside it, and every selected
file (or line of the list) below with what the mask makes of it, recomputed as
you type.

**Tag blocks are visible and changeable.** One file can carry several answers
to the same question — an ID3v2 block and a stale ID3v1 one — which is why a
track can read differently in two programs. A Tag types column and a line in
the editor name what a file holds and which block is being read and written;
a spare block can be dropped; and the tags can be converted to another kind of
block, or an ID3v2 one moved between 2.3 and 2.4, which restamps the header and
keeps every frame rather than rebuilding it. All staged, all undoable.

**Cover art** — fetch from a provider, embed, export, resize on embed, save
`folder.jpg` next to the tracks, or drag an image onto the window.

**BPM from the audio.** Select files and the metronome on the table toolbar
measures their tempo with a neural beat tracker that runs inside the app, then
stages it in the BPM field like any other change. A range setting halves or
doubles the result for libraries that write a 174 BPM drum & bass track as 87.

**A preview player** — gapless playback with prev/next, a three-state repeat and
volume, for checking that a file is what its tags claim. The seek bar draws the
track's loudness envelope, so an intro, a breakdown and a drop are three
different shapes to aim at. On macOS the media keys, the Now Playing widget and
headphone controls drive it too; on Linux the media keys, the desktop's media
widget and `playerctl` do, over MPRIS; on Windows the media keys and the media
flyout do.

**Vinyl-aware** — side letters map to disc numbers (`A1` → disc 1, track 1)
rather than being stored verbatim, with a `MediaType` tag, a render-only
`%side%` mask placeholder and a derived Position column.

**Formats** — MP3, FLAC, Ogg Vorbis, Opus, Speex, M4A/MP4, AAC, AIFF, WAV,
Musepack, Monkey's Audio, WavPack. ID3v2 writes go through the concrete tag type
so DJ cue points, ratings and ReplayGain frames survive a round-trip.

**Comfort** — an interface in English, Deutsch, Español, Français, Italiano,
Українська or Русский (or whichever of them your system asks for),
light/dark/auto themes with an accent colour of your choice, keyboard
shortcuts for the main actions that you can rebind, a bundled IBM Plex type set
so text renders identically on every OS, adjustable table density, and a
Settings › LAB section for typography still being trialled.

</details>

<details>
<summary><b>Motivation</b></summary>


For years, [TagScanner](https://www.xdlab.ru/) and [Mp3tag](https://www.mp3tag.de/en/)
have been the reference tools for putting large music collections in order.
Both are excellent — and both are effectively Windows-only. TagScanner has no
macOS or Linux version and none is planned; Mp3tag on macOS is a paid App Store
port and there is no Linux version at all.

Anyone who migrated from Windows to Apple Silicon or Linux with years of muscle
memory in these tools is left with a fragile chain of workarounds: virtual
machines, ARM Windows builds, x86 emulation. TagRex aims to remove that chain —
one free, open-source editor with the same core workflow on Windows, macOS,
and Linux.

</details>

<details>
<summary><b>Non-goals</b></summary>


- **Not an auto-tagger.** TagRex is a precision tool for people who want to see
  and control every change. For fully automatic DJ-library tagging, see the
  excellent [One Tagger](https://onetagger.github.io/).
- **Not a library manager.** The built-in player is there to check a file, not
  to listen to a collection; TagRex keeps no database of your music and no
  playback history.
- **Not an audio processor.** No format conversion, no ReplayGain analysis
  (at least initially — see the architecture doc for what is deferred).
- **Not a lyrics editor.** Lyrics a file already carries read and write like any
  other tag field, but there is no multi-line editor for them and no batch
  import — that is a listening feature, and this is a tool for metadata in bulk.
- **Not a CUE splitter.** Importing a CUE sheet only pays off if the continuous
  mix it describes is cut into tracks, which is an audio operation rather than a
  metadata one. Writing a CUE *out* of a file list is a different matter.

</details>

<details>
<summary><b>Build from source</b></summary>


Needs a Rust toolchain and the Tauri CLI. **Node is not required** — the
frontend is static HTML/CSS/JS with no build step.

```bash
cargo install tauri-cli --version "^2"
cargo tauri build
```

For a faster iteration build that skips the installer packaging:

```bash
cargo tauri build --debug --bundles app
```

Linux additionally needs the usual WebKitGTK development packages; see the
[Tauri prerequisites](https://tauri.app/start/prerequisites/).

</details>

<details>
<summary><b>Tech stack</b></summary>

A Rust core with a [Tauri](https://tauri.app/) shell and a plain HTML/CSS/JS
interface — no frontend framework and no Node toolchain. The libraries doing the
heavy lifting:

- [lofty](https://github.com/Serial-ATA/lofty-rs) — reading and writing tags in
  the supported formats.
- [rodio](https://github.com/RustAudio/rodio) with
  [Symphonia](https://github.com/pdeljanov/Symphonia) — the preview player and
  the waveform.
- ["Beat This!"](https://github.com/CPJKU/beat_this) through
  [beat-this-rs](https://github.com/danigb/beat-this-rs) on the
  [rten](https://github.com/robertknight/rten) runtime: BPM detection. The
  embedded model weights are under the MIT License; the notice is in
  [crates/player/models/LICENSE](crates/player/models/LICENSE).
- [SQLite](https://www.sqlite.org/) through
  [rusqlite](https://github.com/rusqlite/rusqlite) — the undo journal that
  survives a restart.
- [IBM Plex](https://github.com/IBM/plex) for the interface and
  [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) for file names
  and tag values — the bundled fonts, both under the
  [SIL Open Font License 1.1](https://openfontlicense.org/); the licence texts
  are in
  [app/ui/assets/THIRD-PARTY-FONT-LICENSES.md](app/ui/assets/THIRD-PARTY-FONT-LICENSES.md).

See [docs/architecture.md](docs/architecture.md) for the module layout and the
reasoning behind it.

</details>

<details>
<summary><b>Attribution</b></summary>


TagRex reads metadata from the third-party sources you choose. Their data belongs
to them and to the people who contribute it.

This application uses Discogs' API but is not affiliated with, sponsored or
endorsed by Discogs. "Discogs" is a trademark of Zink Media, LLC.

</details>

<details>
<summary><b>License</b></summary>

[GPL-3.0](LICENSE). Free software stays free: forks and derivatives must remain
open source.

</details>
