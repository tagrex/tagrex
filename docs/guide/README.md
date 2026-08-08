# TagRex user guide

How to use TagRex day to day. If you are looking for how the code is put
together, that is [docs/architecture.md](../architecture.md); for what changed
between releases, [CHANGELOG.md](../../CHANGELOG.md).

## The one idea worth reading first

TagRex does not write to your files when you press a button. Every mutating
operation — a tag edit, a rename, a folder move, a transform, an import, a cover
embed — produces a **change plan**, which is drawn as a diff *into the file
table itself*. You look at it, tick off the rows you want, and press **Apply**.
Only then is anything written, and even then it goes into an undo journal that
survives closing the app.

Once that clicks, the rest of the app is small: a table, five modes that each
propose changes to it, and one Apply/Discard gate they all pass through. If you
read nothing else, read [Change plans, Apply and Undo](change-plans.md).

## Contents

1. [Getting started](getting-started.md) — open a folder, make one edit, undo it
2. [The file table](file-table.md) — columns, grouping, filtering, selection, the player
3. [Change plans, Apply and Undo](change-plans.md) — the preview gate and the undo journal
4. [TAGGER](tagger.md) — edit tags by hand, or pull them from an online source
5. [RENAMER](renamer.md) — rename files and reorganize them into folders
6. [Mask reference](masks.md) — the placeholder language used by RENAMER and reports
7. [GENERATOR](generator.md) — text transforms, action groups, track numbering
8. [DEDUPLICATOR and EXPORTER](duplicates-and-export.md) — find duplicates, write playlists and reports
9. [Settings](settings.md) — every preference, and where they are stored

## Conventions in this guide

- **⌘** means Command on macOS and **Ctrl** on Windows and Linux. Where a
  shortcut differs, both are given.
- Names in **bold** are things you can click. Names in `code` are literal text
  you type — a mask, a filter query, a field name.
- "The selection" always means the rows currently selected in the file table.
  Almost every operation acts on it, and every panel that does says how many
  files it is about to touch.
