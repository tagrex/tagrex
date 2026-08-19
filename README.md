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

> **Status: 0.3.x.** Usable day to day: table editing, rename masks, text
> transforms, online lookups, cover art, duplicate detection, exports, and a
> transactional undo journal. Not 1.0, so expect rough edges — bug reports and
> feedback are welcome. How to use it is in the
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

**The table is the subject.** Files load into a spreadsheet-style table with
configurable tag columns, grouping by any modeled field, filtering (substring or
regex, optionally field-scoped as `artist:aphex`) and saved filter/sort presets.
Selection is first-class: click, ⌘/Ctrl, Shift ranges, keyboard. Tag cells edit
in place.

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
  auto-numbering on import.
- **RENAMER** — rename files and reorganize them into folders from a mask
  (`%artist% - %title%`), with conditional `[...]` sections, zero-padding and
  `%field:width%`. Folder moves create and clean up directories, and same-named
  sidecar files (`.lrc`, `.cue`, per-track covers…) travel with the track.
- **GENERATOR** — text transforms: case conversion, find/replace, remove
  diacritics, transliterate Cyrillic and Greek to Latin, musical ⇄ Camelot key
  notation. Chains can be saved as named action groups and re-run as one plan.
- **DEDUPLICATOR** — read-only scan for likely duplicates by a chosen criterion.
- **EXPORTER** — M3U playlists, CSV, HTML, XML, and mask-based reports.

**Cover art** — fetch from a provider, embed, export, resize on embed, save
`folder.jpg` next to the tracks, or drag an image onto the window.

**A preview player** — gapless playback with prev/next, a three-state repeat and
volume, for checking that a file is what its tags claim.

**Vinyl-aware** — side letters map to disc numbers (`A1` → disc 1, track 1)
rather than being stored verbatim, with a `MediaType` tag, a render-only
`%side%` mask placeholder and a derived Position column.

**Formats** — MP3, FLAC, Ogg Vorbis, Opus, Speex, M4A/MP4, AAC, AIFF, WAV,
Musepack, Monkey's Audio, WavPack. ID3v2 writes go through the concrete tag type
so DJ cue points, ratings and ReplayGain frames survive a round-trip.

**Comfort** — light/dark/auto themes, a bundled IBM Plex type set so text renders
identically on every OS, adjustable table density, and a Settings › LAB section
for typography still being trialled.

## Not yet

- **Parsing tags out of filenames.** The mask grammar is bidirectional in the
  core and `%skip%` exists for it, but only the render direction (tags →
  filename) is exposed in the UI.
- **A waveform seek bar**, lyrics, multi-value fields, AcoustID fingerprinting,
  CUE import/export, and a multilingual UI.

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
