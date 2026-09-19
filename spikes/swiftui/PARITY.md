# SwiftUI stand ↔ Tauri UI — parity audit

**Reference (the spec):** the shipping Tauri frontend in `app/ui/js/*.js`.
**Subject:** the SwiftUI stand in `spikes/swiftui/Sources/TagRexSpike/*.swift`.
**Method:** each stand mode compared to its Tauri module by behaviour, not by
line count.

## Verdict

**Update (2026-09-19): the parity push landed every P1 gap and most of the P2s.**
The stand is now a real second interface, not a demonstration subset. The whole
`crates/ffi` command surface is wired (`read_cover_image` and
`builtin_action_groups`, flagged below as "missing", were in fact already in the
dispatcher). Each mode's ✅ rows below say what shipped.

**Remaining:** cell autocomplete (needs an editable table), column order/width/
custom columns (T2 shipped visibility), G-4 ticked multi-group runs (over-plan
shipped), and assorted P3 polish.

The original verdict, for history: the stand was built free-hand, a happy-path
subset of the Tauri modules; almost every gap was UI-side, since the dispatcher
already exposed the commands.

Severity: **P1** = core behaviour of the mode is missing/wrong; **P2** =
significant feature absent; **P3** = polish/consistency.

---

## 1. Online — `online.js` (1377) vs `Online.swift` (317)

The mode the user flagged. The stand was built free-hand.

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| O1 | Default source **MusicBrainz** (`source = .musicbrainz`, Online.swift:13) | Default **Discogs** (`releaseSource = "discogs"`, online.js:40) | P1 | n/a |
| O2 | Source is a **segmented** control (Online.swift:54) | Source is a **dropdown** `online-source` | P2 | n/a |
| O3 | **Three** fields Artist/Album/Catalogue (Online.swift:60–62) | **One** query field `discogs-query` (online.js:114) | P1 | ready |
| O4 | Fields never prefilled | **Query presets from the selection** (#97): `presetSourceTrack()` = first selected row, `queryFromPreset()` builds text from tags; each preset row shows the **actual text** it will search (online.js:336–383) | P1 | ready |
| O5 | Release card has **no cover** (Online.swift:140) | Card shows `.release-cover` + `.media-badge`, lazy-loaded | P1 | `provider_fetch_image` ready; embed path uses `read_cover_image` — **missing in dispatcher** |
| O6 | Header text = artist · year · country only | Tauri release formatting (label, catalogue, format, track count `.tk-count`) | P2 | ready |
| O7 | No format filter | `search-format` filter passed into query (online.js:149) | P3 | ready |

## 2. Tagger / Editor — `editor.js` (756) vs inspector in `App.swift`

The stand editor is a fixed 7-field form (Artist, Title, Album, Album artist,
Year, Genre, Track) + read-only File block. The Tauri editor is dynamic.

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| E1 | ✅ **Done** — dynamic field editor over the file's real tags, grouped Core / Standard / Advanced with duo (n/total) rows and inline numeric validation (`EditorFields.swift`) | **Dynamic field editor** over the file's real tags, grouped into collapsible sections (`renderFieldEditor`, `fieldGroup`, editor.js:431/487) | P1 | ready (`list_tracks` carries the tags) |
| E2 | ✅ **Done** — a "Remove <block>" per spare block the selection carries, with a loss confirmation for an inexact strip (`TagBlocks.swift`) | **Tag blocks** shown with **strip** buttons (`preview_remove_tag_block`, editor.js:130/159) | P1 | `preview_remove_tag_block` ready |
| E3 | ✅ **Done** — a Convert picker (kind + ID3v2 revision) over the read block, gated to a single read kind, with a per-file loss confirmation (`TagBlocks.swift`) | **Convert a block** between kinds / ID3 revisions (`tag_block_targets`, `preview_convert_tag_block`, editor.js:192/218/292) — the #47/#205 feature | P1 | both ready |
| E4 | ✅ **Done** — an Add-field row that stages an arbitrary custom frame across the selection (`App.swift`) | **Add an arbitrary field** (`openAddField`, `addCustomField`, `populateKnownFields`, editor.js:353/373/693) | P2 | ready (`preview_tag_edits`) |
| E5 | ✅ **Done** — the dynamic editor edits the whole selection, shows `<multiple values>` where it disagrees and stages across all | **Multi-file editing** with a per-field count "— N files" and mixed-value handling (`refreshFieldEditor`, editor.js:40) | P2 | ready |
| E6 | No paired rows | **Duo rows** — track/total etc. on one line (`fieldDuoRow`, editor.js:602) | P3 | ready |
| E7 | No validation feedback | **Per-field validation** (`validateFieldValue`, editor.js:417) | P3 | ready |
| E8 | No cover well | see §9 (cover editing entirely absent) | P1 | mostly ready |

## 3. Generator — `generator.js` (464) + `chain.js` (696) + `chains.js` vs `Generator.swift` (211)

The stand is **one rule**. The Tauri generator is a rule-chain engine.

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| G-1 | ✅ **Done** — a chain editor: add/remove/reorder/enable steps, each with a per-rule scope override, run as one action group through `preview_transform_groups`; the Presets menu loads the shipped builtin chains **and the user's own saved groups**, which the chain saves to and deletes from settings.json (`Generator.swift`, `Library` action-group round-trip). | **Chains of rules** — saved & builtin action groups, group menus, a chain editor (`createRuleChain`, `initActionGroups`, `initBuiltinGroups`, chain.js) | P1 | `preview_transform_groups`/`_over_plan` + `builtin_action_groups` ready |
| G-2 | ✅ **Done** — a Number sub-tab: start value, write-total, optional disc, numbers the selection in table order (`Generator.swift`, `Library.numberTracks`) | **Number tracks** (`numberTracks`, generator.js:125) | P2 | ready (builds a `preview_transform_groups` payload) |
| G-3 | ✅ **Done** — a Vinyl sub-tab that splits an A1/B2 track tag into a disc + track number (`Generator.swift`, `Library.splitVinylSides` + `parseVinylPosition`) | **Split vinyl sides** A/B (`splitVinylSides`, generator.js:197; vinyl.js) | P2 | ready |
| G-4 | ✅ **Done (over-plan)** — when a plan is staged, an "Apply chain to staged changes" button layers the chain over it via `preview_transform_over_plan` (`Generator.swift`, `Library.transformOverStagedPlan`). Ticked multi-group runs still to come. | Transform **over a staged plan** and over **ticked groups** (`preview_transform_over_plan`, `runTickedGroups`, generator.js:233/259) | P2 | ready |
| G-5 | ✅ **Done** — Stage is disabled when the preview finds nothing to change | `nothingChanged()` guards the run (generator.js:106) | P3 | n/a |

## 4. Renamer — `renamer.js` (170) vs `Renamer.swift` (132)

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| R1 | ✅ **Done** — a Move-into-folders sub-mode: folder pattern, destination picker, Move/Copy, prune-empty toggle, and a preview of each file's new folder path (`Renamer.swift`) | **Move / reorganise into folders** — move modes + destination picker (`previewMove`, `setMoveMode`, `pickDestination`, renamer.js:62/96/127) | P1 | `preview_move` ready |
| R2 | ✅ **Done** — the rename and move masks persist across sessions via `@AppStorage` (`Renamer.swift`) | Mask + destination **persisted** (`writeStored`/`readStored`, renamer.js:108) | P3 | n/a |

## 5. From name — `fromname.js` (168) vs `FromName.swift` (143)

Closest to parity, but:

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| F1 | ✅ **Done** — a "Clean up captured values" chain (the shared `ChainEditor`) runs the captured fields through `preview_transform_over_plan` before staging (`FromName.swift`) | Captured fields run **through a transform chain** before staging (`throughChain`, fromname.js:86) | P2 | ready |
| F2 | ✅ **Done** — the from-name mask persists across sessions via `@AppStorage` (`FromName.swift`) | Mask **persisted** (`loadFromNamePrefs`/`saveFromNamePrefs`, fromname.js:29/38) | P3 | n/a |
| F3 | ✅ **Done** — Stage is disabled unless the probe matches the name | guard staging when the probe does not match | P3 | n/a |

## 6. Deduplicator — `dedup.js` (88) vs `Duplicates.swift` (124)

Roughly at parity for the **scan** (criteria + grouped render + size/time
formatting). Gap: acting on a group.

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| D1 | ✅ **Done** — the first file in each group is badged "keep"; a per-file trash button and a group "Trash extras" move the rest to the Trash (`trash_files`), behind a confirmation, then re-scan (`Duplicates.swift`) | **Trash** the redundant files in a group (`trash_files`) | P2 | `trash_files` ready |
| D2 | Two "no duplicates" messages | one empty state | P3 | n/a |

## 7. Exporter — `exporters.js` (139) vs `Export.swift` (119)

Formats match (playlist/cue/csv/html/xml/report). Gap:

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| X1 | ✅ **Done** — a Split control on the playlist format (One / By folder / By album) with a name mask; splitting writes one playlist per group via `export_playlists` (`Export.swift`) | **Split** playlists per folder/album (`setExportSplit`, `export_playlists`, exporters.js:82/100) | P2 | `export_playlists` ready |
| X2 | ✅ **Done** — a one-line hint per export format (`Export.swift` `formatHint`) | Per-kind hint copy (`exportHint`, exporters.js:25) | P3 | n/a |

## 8. File table — `columns.js` (779) + `grouping.js` (88) + `tablegestures.js` + `reorder.js` vs inline table in `App.swift`

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| T1 | ✅ **Done** — rows grouped by folder into `Table` sections, header = the root-relative folder path (`gui-test/CD1`); a toolbar toggle flattens it. On by default like the web UI. | Rows **grouped by folder** with section headers (`groupKeyOf`, `folderGroupLabel`, grouping.js); the v0.15 accent band | P1 | ready (paths are in `list_tracks`) |
| T2 | ✅ **Done (visibility)** — a Columns picker toggles which modeled columns show (Artist/Title/Album/Album Artist/Track/Year/Genre), sortable and persisted via `@AppStorage` (`App.swift`). Order/width/custom columns still to come. | **Configurable columns** — which, order, width, custom (`columns.js`, `render_column`) | P2 | `render_column` ready |
| T3 | Empty-area zebra bands (dark) read as unloaded rows | — | P3 | n/a |
| T4 | — | Table gestures / row reorder (`tablegestures.js`, `reorder.js`) | P3 | — |

## 9. Absent surfaces (whole features with no stand UI)

| Area | Tauri | Sev | Backend |
|------|-------|-----|---------|
| **Cover editing** | ✅ **Done** — a cover well showing the selection's shared/mixed/absent artwork, with Replace (pick an image → `read_cover_image` → `preview_cover_set`), Remove (`preview_cover_remove`) and From-folder (`read_external_cover`) (`Cover.swift`) | `cover.js` (499): choose/embed/add/remove cover, external-cover detection, cover well (`preview_cover_set/embed/remove`, `read_external_cover`, `read_cover_summary`) | P1 | ready (`read_cover_image` now in the dispatcher) |
| **Settings screen** | `settings.js` (425) + `prefs.js` (315): Discogs token, proxy, rate limit, ID3 revision, display size, … | P1 | `load_settings`/`save_settings`/token commands ready |
| **Field locks** | ✅ **Done** — a padlock beside each editor field toggles a session-wide lock (`set_locked_fields`); a locked field dims, goes inert, and every plan skips it (`App.swift`, `Library.swift`) | `locks.js` (103): lock a field so every plan skips it (`set_locked_fields`, `locked_fields`) | P2 | ready |
| **Cell autocomplete** | `suggest.js` (250): inline cell editing with suggestions | P2 | — |
| **Player** | ✅ **Done** — a waveform seek bar (1000 `waveform` buckets, played portion tinted, click/drag to seek), a now-playing cover, and a repeat button cycling off/all/one that drives the gapless advance (`Player.swift`, `Library.swift`) | `player.js` (618) vs `Player.swift` (116): **waveform** canvas, now-playing cover, repeat modes, themed peaks (`waveform`, `read_cover_summary`, `applyRepeatMode`) | P2 | `waveform`/`read_cover_summary` ready |

## 10. Missing dispatcher commands (backend work, small)

- ~~`read_cover_image`~~ — now in the dispatcher (used by §9 cover editing).
- ~~`builtin_action_groups`~~ — now in the dispatcher (for the generator's builtin chains).

Both commands the audit flagged as missing have since been added to
`crates/ffi/src/lib.rs`, so the whole command surface is now reachable from Swift.

## Suggested order

1. **Online to parity** (O1–O7) — flagged, self-contained, all backend-ready bar O5's embed path.
2. **Editor: tag blocks + convert + dynamic fields** (E1–E4) — the tag-block story is a headline feature (#47/#205) and entirely missing.
3. **Table: folder grouping** (T1) — changes how every mode reads.
4. **Cover editing** (§9) + `read_cover_image` — needed by Online too.
5. **Settings screen** — unblocks Discogs token / proxy / ID3 revision from inside the app.
6. Renamer move (R1), Generator chains (G-1…G-4), Export split (X1), Dedup trash (D1), Player waveform.
7. Locks, autocomplete, column config, the P3 polish.
