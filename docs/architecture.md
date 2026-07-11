# TagRex architecture

This document fixes the module boundaries and the reasoning behind them before
any code exists. It is the yardstick for future design decisions and the first
thing a contributor should read.

## The one idea everything follows from

Every operation in a tag editor — renaming from a mask, parsing tags out of a
filename, importing a Discogs release, running a regex replace — is the same
thing: a **plan of changes** applied to a set of files. TagScanner's defining
quality, the one this project exists to reproduce, is that the user always sees
the plan rendered as a "current → new" preview *before* anything touches disk.

Therefore the heart of TagRex is a single transactional pipeline:

```text
source of changes ──> plan ──> preview ──> apply ──> undo journal
```

Everything else — masks, transforms, online providers — is just a producer of
plans. No module writes tags or renames files directly; all writes go through
the pipeline. This is an invariant, not a preference.

## Module map

### Core (single Rust workspace member, always compiled)

| Module | Responsibility | Notes |
| --- | --- | --- |
| Tag engine | Read/write ID3v2.3/2.4, Vorbis Comments, MP4 atoms | Backed by [lofty](https://github.com/Serial-ATA/lofty-rs); no hand-written parsers |
| Scanner and table model | Async directory scan, virtualized list, multi-select | Must stay responsive at 50k+ files |
| Mask engine | One grammar, two directions: generate (tags → filename) and parse (filename → tags) | A single implementation for both directions is mandatory — divergent placeholder behavior between rename and import is the worst class of bug this tool can have |
| Transform pipeline | Case conversion, find/replace, regex, trimming | Composable chain of steps over fields; used by masks, manual edits, and provider post-processing (e.g. stripping Discogs `Artist (3)` disambiguation) |
| Transaction pipeline | Compile plans, render previews, apply, journal | See "Undo journal" below |

### Plugins (module boundary now, dynamic loading maybe later)

"Plugin" initially means a **trait boundary in a separate crate**, not dynamic
loading. Rust dynamic plugins (stable ABI, `dlopen`) are pain, and a WASM
runtime is over-engineering at this stage. All providers compile into the
binary but live in isolated crates behind a common trait:

```rust
// Sketch, not final. Lives in core; providers implement it in their own crates.
trait MetadataProvider {
    fn search(&self, query: &SearchQuery) -> Result<Vec<ReleaseCandidate>>;
    fn fetch_release(&self, id: &ReleaseId) -> Result<Release>;
    fn field_mapping(&self) -> &FieldMapping;
}
```

Why this boundary matters: metadata sources die. Beatport closed its public
API and TagScanner had to remove the feature entirely. With providers as
isolated crates, a dead API kills one crate — the core is untouched, and the
community can add or fix sources through ordinary pull requests independent of
the core release cycle.

Planned plugin families:

- **Metadata sources.** Discogs first (personal user token, 60 req/min is
  plenty for tagging), MusicBrainz second (no token, better for non-electronic
  music). Bandcamp and others can follow as contributions.
- **Cover sources.** Fetch, embed, export.
- **Exporters.** Playlists, CSV, text reports.

### Deferred (deliberately out of scope for now)

- **Audio fingerprinting (AcoustID).** That is Picard's territory; the first
  versions prioritize manual control over automagic. Also drags in a
  Chromaprint dependency.
- **ReplayGain.** Audio analysis, not metadata editing. Different world.
- **Scripting / actions** (Mp3tag-style). Deferred, but not ignored: the
  transform pipeline is designed as a composable chain precisely so that
  scripting later becomes *serialization of chains into saved presets*, not a
  new subsystem.
- **Duplicate finder, exotic formats** (APE, WavPack, ASF). lofty supports the
  formats, enabling them is cheap, but they earn no UI priority yet.

## Undo journal

The one place where extra complexity is bought up front. The journal must:

- persist across application restarts (SQLite file next to the config);
- record both tag writes (old value → new value per field) and renames
  (old path → new path);
- allow rollback of a whole batch as a unit.

Rationale: "renamed 8,000 files, closed the app, realized in the morning the
mask was wrong" is a real scenario, and it is exactly where every existing
editor fails.

## Tech stack

- **Core:** Rust. Batch operations over tens of thousands of files are the hot
  path; native speed is not optional.
- **Tag I/O:** lofty. Writing ID3/Vorbis/MP4 parsers by hand is years of work
  already done elsewhere.
- **Shell:** Tauri. One codebase for Windows, macOS (including Apple Silicon),
  and Linux; small binaries; the Rust core runs in-process.

## Implementation order

1. Core without any network: scanner → table → masks → preview → apply →
   undo. Formats: MP3, FLAC, M4A. This alone is already "TagScanner without
   the internet" and a usable tool.
2. `MetadataProvider` trait + Discogs provider (personal token).
3. MusicBrainz provider.
4. Covers and exporters.
5. Revisit the deferred list.

## Prior art and positioning

- **TagScanner, Mp3tag** — the reference UX; Windows-only (Mp3tag's macOS port
  is paid, no Linux for either).
- **Kid3** — cross-platform and capable, but the interface shows its age.
- **MusicBrainz Picard** — superb at fingerprint-driven auto-tagging, weak at
  manual batch surgery.
- **puddletag** — the Mp3tag clone for Linux; Linux-only.
- **One Tagger** — open source, cross-platform, Rust: the automation
  counterpart. TagRex is the manual precision counterpart, not a competitor.
