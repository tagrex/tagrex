# Native responsiveness and recovery

This contribution keeps the static interface and its build requirements. There
is no npm build step. The small ES-module artwork codec preserves complete
inline images for callers while the command transport shares repeated payloads.

Changes include:

- Queue blocking session, settings and player work away from the UI thread.
  Cancellation bypasses the session queue.
- Reuse provider clients with independent provider queues and existing limits.
- Cache metadata against filesystem fingerprints, batch custom-column reads
  and expose scoped row readback.
- Stream duplicate-file hashing instead of loading a whole file into memory.
- Share immutable artwork in memory and during command transport, and keep
  history listing free of snapshot hydration.
- Keep interrupted batches when automatic rollback is incomplete, including
  failures while finalising the journal.
- Publish artwork/export files through guarded staging, preserving recovery
  backups when restoration fails.
- Preserve exact provider artist/label identities for opening source pages.

## Undo storage compatibility

New cover and tag-block image records retain the original inline BLOB columns.
There is no image-reference table, reference column or sentinel in the journal.
Artwork pooling on disk is deferred for separate discussion. Loaded images can
still share memory after their inline bytes have been read.

`crates/core/examples/undo_compat.rs` is a synthetic cross-version smoke tool:

1. Run `write /absolute/path/to/a/new/fixture-directory` with this branch.
2. Copy the same example into a clean stable checkout's `crates/core/examples`.
3. Run `read-undo` against that directory using the stable checkout.

Both invocations use `cargo run --locked -p tagrex-core --example undo_compat --`.
The writer refuses an existing fixture directory. The reader verifies two
persistent batches, exact cover bytes, types, descriptions and ordering, then
uses the stable executor to restore the original tags and artwork.

This passed against stable commit `a092456`. It tests the stable Rust reader and
executor, rather than inferring compatibility from a query against the new code.

## Validation

- Native workspace: 683 tests passed, 13 ignored.
- Whole-workspace Clippy, all targets/features with warnings denied, and fmt pass.
- Five standalone JavaScript codec tests pass.
- The unbuilt static interface boots in an isolated browser fixture, opens a
  library and previews an Editor edit without writing a file.
- Cross-version journal creation, loading and Undo pass as described above.

The capacity probe retains 1,000-file preview/clone coverage but bounds its
journal section to four files because compatible inline snapshots intentionally
store repeated artwork. Its storage footprint is not presented as deduplicated.

These are synthetic checks on Linux. Large-library interaction benchmarks,
physical-GPU measurements and Windows/macOS runtime validation remain to do.
