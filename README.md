<p align="center">
  <img src="assets/logo.svg" width="120" alt="TagRex logo">
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/wordmark-dark.svg">
    <img src="assets/wordmark-light.svg" width="320" alt="/tagrex/">
  </picture>
</p>

<p align="center">
  Cross-platform audio tag editor. Table editing, bidirectional masks, transactional undo.
</p>

---

> **Status: 0.14.x.** Usable day to day: table editing, rename masks with a
> function language, tags read back out of file names, text transforms, online
> lookups, cover art, tag block conversion, duplicate detection, exports, a
> preview player, a transactional undo journal, and an interface that speaks
> English, Ukrainian and Russian. Not 1.0, so expect rough
> edges — bug reports and feedback are welcome. How to use it is in the
> [user guide](docs/guide/README.md); the design is written up in
> [docs/architecture.md](docs/architecture.md); user-visible changes are in
> [CHANGELOG.md](CHANGELOG.md).

## Install

Builds are on the [Releases](https://github.com/tagrex/tagrex/releases) page.

| Platform | Architectures | Package |
| --- | --- | --- |
| macOS | Apple Silicon | `.dmg` |
| Windows | x86-64, ARM64 | `.exe` installer |
| Linux | x86-64, ARM64 | `.deb`, `.rpm` |

Intel Macs are not built — Apple Silicon only.

## What it does

**A folder is the library.** Point TagRex at one and it recurses: type or browse
to the path, drop the folder on the window, or hand it over from the file
manager — **Open With**, the Dock icon, or the command line.

**The table is the subject.** Files load into a spreadsheet-style table with
configurable tag columns, grouping by any modeled field, filtering (substring or
regex, optionally field-scoped as `artist:aphex`) and saved filter/sort presets.
Selection is first-class: click, ⌘/Ctrl, Shift ranges, keyboard. Tag cells edit
in place, offering the values the library already holds as you type. A button
beside the folder path re-reads what is open without losing your sort, filter or
pending edits, and right-clicking a file offers to drop it from the list or move
it to the system Trash.

**Nothing is written until you look at it.** Every mutating operation — rename,
move, tag edit, transform, import, cover embed — produces a change plan that is
rendered as a diff *into the table itself*, with a per-row apply scope and an
Apply/Discard bar. Applying goes through a transactional executor with a
persistent SQLite undo journal, so a batch survives an application restart and
can still be rolled back.

**Fields can be locked.** A padlock beside a field in the EDITOR panel puts it
out of reach: no import, transform, rename or clear-tags can change it, its
table cells stop opening for editing, and a plan reports what the lock kept out
rather than silently doing less. Locks last for the session — one you set months
ago and forgot would be worse than none.

**Modes**, each a verb applied to that table:

- **TAGGER** — edit tags by hand, or pull them from Discogs (personal token) or
  MusicBrainz. Paged search, release cards with cover browser and tracklist,
  content-based candidate matching plus exact ISRC matching, auto-align and
  auto-numbering on import. A **FROM NAME** sub-tab runs a mask the other way
  round — the file's own name read back into tags, with `%skip%` for the junk
  that maps to nothing and a live read-out of what the pattern is pulling out.
- **RENAMER** — rename files and reorganize them into folders from a mask
  (`%artist% - %title%`), with conditional `[...]` sections, zero-padding,
  `%field:width%` and the function language below. Folder moves create and clean
  up directories, and same-named sidecar files (`.lrc`, `.cue`, per-track
  covers…) travel with the track.
- **GENERATOR** — text transforms: case conversion, find/replace, remove
  diacritics, transliterate Cyrillic and Greek to Latin, musical ⇄ Camelot key
  notation. Every rule names the field it acts on, so one chain can upper-case a
  catalogue number while title-casing the titles, and chains can be saved as
  named action groups and re-run as one plan.

**Cleanup is part of the job, not a second step.** Importing a release, reading
tags out of a file name and renaming each carry a rule chain of their own —
set up in a dialog behind the wand, remembered between runs — and it runs as
part of that panel's own button: one press, one plan, one undo entry. The
targets differ enough that sharing one chain was the bug: RENAMER usually wants
a space turned into an underscore and FROM NAME wants exactly the opposite. The
tag editor has none on purpose — a value typed by hand comes out as typed.
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
argument.

**Tag blocks are visible and changeable.** One file can carry several answers
to the same question — an ID3v2 block and a stale ID3v1 one — which is why a
track can read differently in two programs. A Tag types column and a line in
the editor name what a file holds and which block is being read and written;
a spare block can be dropped; and the tags can be converted to another kind of
block, or an ID3v2 one moved between 2.3 and 2.4, which restamps the header and
keeps every frame rather than rebuilding it. All staged, all undoable.

**Cover art** — fetch from a provider, embed, export, resize on embed, save
`folder.jpg` next to the tracks, or drag an image onto the window.

**A preview player** — gapless playback with prev/next, a three-state repeat and
volume, for checking that a file is what its tags claim. The seek bar draws the
track's loudness envelope, so an intro, a breakdown and a drop are three
different shapes to aim at.

**Vinyl-aware** — side letters map to disc numbers (`A1` → disc 1, track 1)
rather than being stored verbatim, with a `MediaType` tag, a render-only
`%side%` mask placeholder and a derived Position column.

**Formats** — MP3, FLAC, Ogg Vorbis, Opus, Speex, M4A/MP4, AAC, AIFF, WAV,
Musepack, Monkey's Audio, WavPack. ID3v2 writes go through the concrete tag type
so DJ cue points, ratings and ReplayGain frames survive a round-trip.

**Comfort** — an interface in English, Українська or Русский (or whichever of
them your system asks for), light/dark/auto themes, a bundled IBM Plex type set
so text renders identically on every OS, adjustable table density, and a
Settings › LAB section for typography still being trialled.

## Not yet

- **AcoustID** fingerprinting.

## Motivation

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

## Non-goals

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

## Build from source

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

## Tech stack

Rust core ([lofty](https://github.com/Serial-ATA/lofty-rs) for tag I/O,
[rodio](https://github.com/RustAudio/rodio)/Symphonia for playback) with a
[Tauri](https://tauri.app/) shell. See
[docs/architecture.md](docs/architecture.md) for the module layout and the
reasoning behind it.

## License

[GPL-3.0](LICENSE). Free software stays free: forks and derivatives must remain
open source.
