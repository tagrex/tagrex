# Fuzzing

Coverage-guided fuzz targets for the code that reads bytes it did not write
(#719). They run weekly in the `Fuzz` workflow; this is how to run them by hand.

| Target | What it feeds | What counts as a finding |
|---|---|---|
| `tag_read` | any file, as each supported format, to `TagEngine::read` and `read_covers` | a panic that escapes, a hang |
| `tag_write` | a file that reads, written back with a few changed fields | the above, a successful write that leaves the file unreadable, a failed write that changes the file |
| `cover_resize` | any bytes as cover art, to `resize_cover` | a panic, a hang |
| `player_decode` | any file to the player's decode path (`waveform`) | a panic, a hang |

The first byte of every file input picks the extension it is written under
(see `EXTENSIONS` in `src/lib.rs`), so one corpus serves every format.

A panic that the app catches on purpose (the tag library's, inside `guarded`)
is not a finding: `tagrex_fuzz::run` only fails when a panic reaches it.

## Running

Needs a nightly toolchain and cargo-fuzz:

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
```

From the repository root, with the app's own lockfile so the fuzzed dependency
versions are the shipped ones:

```sh
cp Cargo.lock fuzz/Cargo.lock
cargo +nightly fuzz run -O tag_write fuzz/corpus/tag_write fuzz/seeds -- -max_total_time=300 -timeout=10
```

`-O` builds without overflow checks, as the app is released. Without it an
overflow in a dependency that wraps harmlessly in the app shows up as a panic.

A crash leaves its input under `fuzz/artifacts/<target>/`. To see what it does
outside the fuzzer, with the file kept before and after the write:

```sh
cd fuzz
cargo +nightly run --example replay -- artifacts/tag_write/crash-<hash> /tmp/replay
```

Every finding gets its own issue, and its input becomes a regression test next
to the code it broke.

## Seeds

`seeds/` holds one small tagged file per format, so the fuzzer starts inside
tag blocks instead of having to invent their headers. `examples/seeds.rs`
rebuilds it from untagged files:

```sh
cd fuzz
cargo +nightly run --example seeds -- <untagged files...> seeds
```

The untagged files were 50 ms of a sine tone encoded by ffmpeg (MP3, FLAC, Ogg
Vorbis, Opus, M4A, ADTS AAC, WavPack, AIFF, WAV) plus the Speex fixture from
`crates/core/tests/fixtures`, which stays untagged: it is damaged on purpose
and the writer refuses it.

Three more seeds are written by hand, not by `examples/seeds.rs` (#860). They
are MP3s whose ID3v2 tag holds chapter frames: `seed-mp3-chapters` (2.4) and
`seed-mp3-chapters-v23` (2.3) with an ordinary CHAP and CTOC, each holding a
title, and `seed-mp3-nested-chapters` with chapter frames inside one another,
which the app refuses (#859) because the tag library overflows its stack on
them. Each is the selector byte `0` (MP3) followed by the file, so a mutation
can grow an ordinary chapter into a nested one, or the other way round.
