# SwiftUI stand ↔ Tauri UI — parity audit

**Reference (the spec):** the shipping Tauri frontend in `app/ui/js/*.js`.
**Subject:** the SwiftUI stand in `spikes/swiftui/Sources/TagRexSpike/*.swift`.
**Method:** each stand mode compared to its Tauri module by behaviour, not by
line count.

## Verdict

The stand is a **demonstration subset**, not a port. Every mode exercises the
happy path of one or two ABI commands and stops there; the Tauri modules carry
several times more behaviour each. Crucially, **almost every gap is UI-side, not
backend-side**: the `crates/ffi` dispatcher already exposes the whole command
surface (`preview_move`, `export_playlists`, `preview_convert_tag_block`,
`preview_remove_tag_block`, `preview_cover_*`, `set_locked_fields`,
`preview_transform_groups/over_plan`, `tag_block_targets`, `trash_files`,
`render_column`, …). The stand simply never wired those commands into the UI.
Two commands the Tauri UI uses are **not** in the dispatcher yet and would need a
backend line: `read_cover_image` and `builtin_action_groups`.

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
| E1 | Fixed 7 fields | **Dynamic field editor** over the file's real tags, grouped into collapsible sections (`renderFieldEditor`, `fieldGroup`, editor.js:431/487) | P1 | ready (`list_tracks` carries the tags) |
| E2 | No tag-block UI | **Tag blocks** shown with **strip** buttons (`preview_remove_tag_block`, editor.js:130/159) | P1 | `preview_remove_tag_block` ready |
| E3 | No convert | **Convert a block** between kinds / ID3 revisions (`tag_block_targets`, `preview_convert_tag_block`, editor.js:192/218/292) — the #47/#205 feature | P1 | both ready |
| E4 | No custom fields | **Add an arbitrary field** (`openAddField`, `addCustomField`, `populateKnownFields`, editor.js:353/373/693) | P2 | ready (`preview_tag_edits`) |
| E5 | Single-file only | **Multi-file editing** with a per-field count "— N files" and mixed-value handling (`refreshFieldEditor`, editor.js:40) | P2 | ready |
| E6 | No paired rows | **Duo rows** — track/total etc. on one line (`fieldDuoRow`, editor.js:602) | P3 | ready |
| E7 | No validation feedback | **Per-field validation** (`validateFieldValue`, editor.js:417) | P3 | ready |
| E8 | No cover well | see §9 (cover editing entirely absent) | P1 | mostly ready |

## 3. Generator — `generator.js` (464) + `chain.js` (696) + `chains.js` vs `Generator.swift` (211)

The stand is **one rule**. The Tauri generator is a rule-chain engine.

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| G-1 | A single rule, applied directly | **Chains of rules** — saved & builtin action groups, group menus, a chain editor (`createRuleChain`, `initActionGroups`, `initBuiltinGroups`, chain.js) | P1 | `preview_transform_groups`/`_over_plan` ready; `builtin_action_groups` **missing in dispatcher** |
| G-2 | — | **Number tracks** (`numberTracks`, generator.js:125) | P2 | ready (builds a `preview_transform_groups` payload) |
| G-3 | — | **Split vinyl sides** A/B (`splitVinylSides`, generator.js:197; vinyl.js) | P2 | ready |
| G-4 | Preview only over files | Transform **over a staged plan** and over **ticked groups** (`preview_transform_over_plan`, `runTickedGroups`, generator.js:233/259) | P2 | ready |
| G-5 | Stage enabled on no-op | `nothingChanged()` guards the run (generator.js:106) | P3 | n/a |

## 4. Renamer — `renamer.js` (170) vs `Renamer.swift` (132)

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| R1 | Rename in place only | **Move / reorganise into folders** — move modes + destination picker (`previewMove`, `setMoveMode`, `pickDestination`, renamer.js:62/96/127) | P1 | `preview_move` ready |
| R2 | Mask not remembered | Mask + destination **persisted** (`writeStored`/`readStored`, renamer.js:108) | P3 | n/a |

## 5. From name — `fromname.js` (168) vs `FromName.swift` (143)

Closest to parity, but:

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| F1 | Captured fields staged as-is | Captured fields run **through a transform chain** before staging (`throughChain`, fromname.js:86) | P2 | ready |
| F2 | Mask not remembered | Mask **persisted** (`loadFromNamePrefs`/`saveFromNamePrefs`, fromname.js:29/38) | P3 | n/a |
| F3 | Stage enabled on no-match | guard staging when the probe does not match | P3 | n/a |

## 6. Deduplicator — `dedup.js` (88) vs `Duplicates.swift` (124)

Roughly at parity for the **scan** (criteria + grouped render + size/time
formatting). Gap: acting on a group.

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| D1 | Read-only scan | **Trash** the redundant files in a group (`trash_files`) | P2 | `trash_files` ready |
| D2 | Two "no duplicates" messages | one empty state | P3 | n/a |

## 7. Exporter — `exporters.js` (139) vs `Export.swift` (119)

Formats match (playlist/cue/csv/html/xml/report). Gap:

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| X1 | One playlist | **Split** playlists per folder/album (`setExportSplit`, `export_playlists`, exporters.js:82/100) | P2 | `export_playlists` ready |
| X2 | — | Per-kind hint copy (`exportHint`, exporters.js:25) | P3 | n/a |

## 8. File table — `columns.js` (779) + `grouping.js` (88) + `tablegestures.js` + `reorder.js` vs inline table in `App.swift`

| # | Stand now | Tauri reference | Sev | Backend |
|---|-----------|-----------------|-----|---------|
| T1 | **No folder grouping** — flat table | Rows **grouped by folder** with section headers (`groupKeyOf`, `folderGroupLabel`, grouping.js); the v0.15 accent band | P1 | ready (paths are in `list_tracks`) |
| T2 | Fixed 5 columns | **Configurable columns** — which, order, width, custom (`columns.js`, `render_column`) | P2 | `render_column` ready |
| T3 | Empty-area zebra bands (dark) read as unloaded rows | — | P3 | n/a |
| T4 | — | Table gestures / row reorder (`tablegestures.js`, `reorder.js`) | P3 | — |

## 9. Absent surfaces (whole features with no stand UI)

| Area | Tauri | Sev | Backend |
|------|-------|-----|---------|
| **Cover editing** | `cover.js` (499): choose/embed/add/remove cover, external-cover detection, cover well (`preview_cover_set/embed/remove`, `read_external_cover`, `read_cover_summary`) | P1 | mostly ready; `read_cover_image` **missing in dispatcher** |
| **Settings screen** | `settings.js` (425) + `prefs.js` (315): Discogs token, proxy, rate limit, ID3 revision, display size, … | P1 | `load_settings`/`save_settings`/token commands ready |
| **Field locks** | `locks.js` (103): lock a field so every plan skips it (`set_locked_fields`, `locked_fields`) | P2 | ready |
| **Cell autocomplete** | `suggest.js` (250): inline cell editing with suggestions | P2 | — |
| **Player** | `player.js` (618) vs `Player.swift` (116): **waveform** canvas, now-playing cover, repeat modes, themed peaks (`waveform`, `read_cover_summary`, `applyRepeatMode`) | P2 | `waveform`/`read_cover_summary` ready |

## 10. Missing dispatcher commands (backend work, small)

- `read_cover_image` — embed a cover read from an arbitrary file (cover.js:140, O5/§9).
- `builtin_action_groups` — the generator's builtin rule chains (chain.js:71, G-1).

## Suggested order

1. **Online to parity** (O1–O7) — flagged, self-contained, all backend-ready bar O5's embed path.
2. **Editor: tag blocks + convert + dynamic fields** (E1–E4) — the tag-block story is a headline feature (#47/#205) and entirely missing.
3. **Table: folder grouping** (T1) — changes how every mode reads.
4. **Cover editing** (§9) + `read_cover_image` — needed by Online too.
5. **Settings screen** — unblocks Discogs token / proxy / ID3 revision from inside the app.
6. Renamer move (R1), Generator chains (G-1…G-4), Export split (X1), Dedup trash (D1), Player waveform.
7. Locks, autocomplete, column config, the P3 polish.
