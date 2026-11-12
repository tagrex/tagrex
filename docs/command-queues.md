# Command scheduling and ordering

Blocking session, settings, image and filesystem work runs on the blocking
pool. Each resource acquires its async permit before spawning work. Provider
sources have separate persistent hubs/queues; cancellation bypasses the queues.
The fetched-image helper extraction preserves the existing output behaviour;
guarded publication is a separate contribution.

## Ordering audit

Audited the static interface at upstream `dae79c6`, including `app.js` and
`js/locks.js`, `settings.js`, `online.js`, `columns.js`, `renamer.js`, `player.js`.

- Library opening awaits `open_library`/`open_drop`, then `pushFieldLocks`, then
  `list_tracks` in `afterOpen`.
- Inline and bulk writes await preview before Apply and await Apply/Undo before
  refreshing rows/history. Rapid field-lock clicks were an ordering dependency:
  separate handlers did not await each other. `pushFieldLocks` now snapshots each
  submission and chains it after the preceding IPC, including after a failure.
- Settings Save awaits token persistence and settings adoption before refreshing
  the library when the parser code page changes.
- Provider image/release fetches are awaited before their result is consumed.
  Custom-column refreshes await each read; those are read-only.
- Fire-and-forget player controls remain synchronous Tauri commands and are not
  moved onto this queue. Fire-and-forget token saving and independent preview
  requests do not establish an ordered session mutation sequence.

The mutex serializes jobs in lock-acquisition order. It **does not guarantee
JavaScript invocation order** across separate IPC requests. The field-lock submission chain fixes the concrete unawaited mutation sequence
found in the current static interface. This is not a protocol-wide ordering
guarantee for future callers. New
code must await dependent mutations or combine them into one native operation.

The settings/open regression deliberately overlaps saving settings with a
blocked library open and checks the newly opened session gets the new values.
Artwork downloads capture the session generation and reject publication if the
library changed while HTTP ran.

## Reproduce on macOS

```
node --experimental-vm-modules --test tests/unit/field-lock-order.test.js
cargo test -p tagrex blocking::tests
cargo test -p tagrex saved_preferences_reach_a_library_that_is_still_opening
cargo test -p tagrex provider_sources_keep_independent_persistent_queues
cargo test -p tagrex saving_network_preferences_does_not_wait_for_inflight_http
cargo tauri build --debug --bundles app
```

Run the debug bundle on a disposable library: open it, lock a field, preview and
apply edits, refresh, then Undo. While a provider request is slow, try another
source and save preferences. During a slow Apply, cancel and check the UI still
repaints and the batch rolls back. Download images, change libraries before the
HTTP request finishes, and check they are not published into the new session.
Native macOS/Windows runtime results are still requested; Linux workspace tests
alone do not establish those behaviours on either platform.
