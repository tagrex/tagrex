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

> **Status: pre-alpha.** Nothing to download yet. The architecture is being laid out
> in [docs/architecture.md](docs/architecture.md) — feedback and discussion are welcome.

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

## Planned features

- Spreadsheet-style table editing with multi-select and inline edits
- Live preview of every batch operation before anything is written
- Bidirectional masks: rename files from tags and parse tags from filenames
  with a single pattern grammar (`%artist% - %title%`)
- Text transforms: case conversion, find and replace, regular expressions
- Transactional apply with a persistent undo journal — batch renames and tag
  writes survive an application restart and can be rolled back
- Online metadata sources as plugins: Discogs (personal token) and MusicBrainz
  first, more sources contributed over time
- Cover art: fetch, embed, export
- Formats at launch: MP3 (ID3v2.3/2.4), FLAC and OGG (Vorbis Comments),
  M4A/MP4

## Non-goals

- **Not an auto-tagger.** TagRex is a precision tool for people who want to see
  and control every change. For fully automatic DJ-library tagging, see the
  excellent [One Tagger](https://onetagger.github.io/).
- **Not a music player or library manager.** It edits metadata and filenames;
  it does not maintain a database of your collection.
- **Not an audio processor.** No format conversion, no ReplayGain analysis
  (at least initially — see the architecture doc for what is deferred).

## Tech stack

Rust core ([lofty](https://github.com/Serial-ATA/lofty-rs) for tag I/O) with a
[Tauri](https://tauri.app/) shell. See
[docs/architecture.md](docs/architecture.md) for the module layout and the
reasoning behind it.

## License

[GPL-3.0](LICENSE). Free software stays free: forks and derivatives must remain
open source.
