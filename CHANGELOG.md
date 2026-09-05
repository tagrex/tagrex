# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **The SwiftUI stand applies an online release to the selected files.** With a
  release open, the Online panel aligns its tracks to the selected files
  (`auto_align`, shown as a per-track match) and a "Stage import" builds the
  import (`preview_import`) and stages it through the same gate a hand edit
  uses — the table shows the changes, the change-plan bar takes over, and Apply
  writes one journaled, undoable batch. Enabled only when every selected file
  matched a track. Spike-only. (#300)
- **The SwiftUI stand searches the online sources.** Its Online sub-tab is a
  working panel over the session ABI: a source picker (Discogs / MusicBrainz /
  Beatport), artist / album / catalogue fields, a results list of release
  candidates, and a release view with the full tracklist. MusicBrainz needs no
  token and works out of the box; Discogs and Beatport surface the provider's
  own auth error until credentials are set. Search-and-look for now — applying a
  release to the files comes next. Spike-only. (#299)

### Changed

- **The command layer is a crate of its own.** It sat in the same crate as the
  desktop shell, which also carries the webview stack and the audio backend, so
  everything the app can do was reachable from that one shell and nothing else.
  It moves to `crates/commands` unchanged — no behaviour, no signatures, no
  logic — which is what lets a second interface call the same backend without
  linking a browser engine to do it. Nothing about the app changes. (#272)
- **The preview player is a crate of its own.** `player.rs` — the rodio Sink on
  its own thread and the waveform decoder — moves to `crates/player` so the
  native shells can drive playback, the same reason the command layer moved
  (#272). The rodio audio backend rides with it. Pure relocation; nothing about
  the app changes. (#296)
- **The native-shell bridge dispatches into the command layer.** `crates/ffi`
  had three hand-written functions, each reimplementing a slice of the app
  against the core. It becomes a session — open a library, then invoke commands
  by name — over the same `App` and the same DTOs the desktop shell uses, so a
  shell reaches the whole synchronous surface (every `preview_*`, the change
  plan, Apply, Undo, the exporters, duplicates, field locks) instead of a
  hand-copied fraction. Spike-only; nothing about the app changes. (#293)
- **The native-shell bridge reaches the online sources.** The session gains a
  provider hub, and the dispatcher gains `provider_search`,
  `provider_fetch_release`, `provider_fetch_image` and `save_release_images` —
  synchronous forwards, no runtime, since the provider crates use blocking HTTP.
  A shell can now search a source, pull a release, and save its cover next to the
  tracks. Spike-only; nothing about the app changes. (#294)
- **The native-shell bridge reads and writes settings.** The session remembers
  its config dir, and the dispatcher gains `load_settings`, `save_settings`,
  `saved_discogs_token` and `save_discogs_token`. Saving applies the settings
  live — proxy and rate limit to the hub, ID3 revision and the rest to the open
  library — so a proxied, rate-limited, token-authenticated Discogs search is now
  drivable from a shell. Spike-only; nothing about the app changes. (#295)
- **The preview player is a crate of its own.** `player.rs` — the rodio Sink on
  its own thread and the waveform decoder — moves to `crates/player` so the
  native shells can drive playback, the same reason the command layer moved
  (#272). The rodio audio backend rides with it. Pure relocation; nothing about
  the app changes. (#296)
- **The native-shell bridge drives the player.** The session gains a `Player`,
  and the dispatcher gains `player_play`, `player_set_next`, `player_pause`,
  `player_resume`, `player_stop`, `player_seek`, `player_set_volume`,
  `player_status` and `waveform`. The device opens lazily on the first play, so
  opening a session touches none. A shell can now play a track, read its state,
  and draw the seek bar. Spike-only; nothing about the app changes. (#297)
- **The native-shell bridge signs in to Beatport.** The dispatcher gains
  `beatport_begin` (scrape the client id, build the authorize URL),
  `beatport_complete` (exchange the redirect's code, persist the session),
  `beatport_status`, `beatport_logout` and `beatport_token` (refresh on expiry).
  The interactive browser half stays with the shell, done natively. This
  completes the command surface for the shells. Spike-only; nothing about the app
  changes. (#298)

## [0.15.0] - 2026-09-05

This release is a visual pass over the workspace. The top bar absorbs the mode
tabs to give the file table back a whole row, folder-group headers and the
online tracklist read more clearly, and the preview player grows up — a
now-playing cover, a grouped transport with an emphasised play/pause, and a bar
that belongs to the file table instead of stretching under the side panel. The
online results drop the Grid layout that only ever collapsed back to the list,
a dragged panel width is remembered between sessions, and an open folder's name
no longer vanishes from the path when you switch language.

### Changed

- **The file table starts one point larger.** The first-run default size is now
  11px instead of 10px — a touch more legible out of the box while keeping the
  dense, information-first character. The Settings › Display control (10–20px)
  is unchanged, so anyone who prefers it tighter drags it straight back to 10.
- **The mode tabs moved up into the top bar.** TAGGER / RENAMER / GENERATOR /
  DEDUPLICATOR / EXPORTER used to sit in their own row below the top bar; folding
  them into it — beside the brand, with the folder path to their right — gives
  the file table that whole row back. On a narrow window the tab labels still
  drop to icon-only before anything crowds, and the path keeps a usable width.
- **Folder-group headers read as a section band.** A group row was set apart only
  by its bold text; it now carries a faint accent wash, thin accent hairlines
  above and below, and its folder name in the accent colour, so the boundary
  between one folder and the next is obvious in a dense table. The wash stays well
  below the selection tint, so a group is never mistaken for a selected row.
- **The artist stands apart from the title in the online tracklist.** In a
  release's track list the artist was grey next to the title; it now takes the
  accent colour, so a various-artists release scans at a glance — title in the
  text colour, artist in green. The dot between them stays neutral.
- **The transport controls read as one cluster.** The player's prev / play-pause
  / stop / next / repeat glyphs are grouped tightly instead of spread evenly
  across the bar, and play-pause now leads them with an accent fill rather than
  looking like one more flat glyph — so the primary control is obvious and the
  row reads as a player.
- **The panel width you drag is remembered.** Dragging the divider between the
  file table and the mode panel used to reset to the default on the next launch;
  the chosen width now persists across sessions. A window too narrow for it still
  shrinks it to fit, and widening the window later brings the full width back.

### Added

- **The player shows the cover of what's playing.** A small thumbnail of the
  current track's embedded front cover now sits at the head of the player bar, so
  the bar reads as a player at a glance. A track with no embedded art leaves the
  slot empty (reserved, so the transport never shifts); the cover follows every
  track change, including gapless auto-advance.
- **The player belongs to the file table now.** The player and its waveform used
  to stretch the full width of the window, running under the side panel; they now
  sit in a left zone that tracks the table's width, so the waveform ends where the
  table does. The selection count moved under the panel — pinned to the panel's
  left edge, with Preview edits at the right — leaving the player its own clean
  strip. In the player, the controls lead: transport, volume, then the cover, then
  the title and waveform.

### Fixed

- **The player now actually shows a track's embedded cover.** The now-playing
  thumbnail was reading covers through the wrong call and never found a track's
  own art, so it always fell back to the placeholder; it now reads the embedded
  front cover the same way the editor's cover well does.
- **The open folder's name survives a language change.** Switching language with
  a folder open could leave the path indicator reading `…/parent/No folder open` —
  the static-text pass overwrote the folder name with the placeholder. The name
  is dynamic again and the indicator repaints on a language change, so it keeps
  the real folder (and the empty-state placeholder still localizes).

### Removed

- **The online results Grid layout is gone.** The List/Grid toggle offered a
  cover-wall view that collapsed straight back to the list — expanded on the
  clicked release — the moment you touched a tile, so it never really was a
  second way to work. List (with per-card expand) is now the single layout, and
  the toggle, the tile rendering and its cover-fetch path go with it.

## [0.14.0] - 2026-08-23

This release is about the words. The interface no longer speaks only English:
strings live in per-language catalogues that Russian and Ukrainian fill in
full, and the text the backend composes — every failure, every plan
description, the whole placeholder reference — now travels as a code and its
values, so it reads in the language of the day rather than the one it was
written in. EXPORTER gained range alongside it: a CUE sheet out of the file
list, and a playlist that can come out one per folder or per album. Two fixes
close the release — a message that stopped repeating the path already on
screen, and exported paths that now survive being carried to another platform.

### Added

- **The interface can speak Russian, starting with EXPORTER.** Settings ›
  Display gains a **Language** control — Auto, English, Русский — beside the
  theme, and it switches live. The machinery is in place for the rest of the
  app: strings live in per-language catalogues, a missing entry falls back to
  English rather than showing a blank or an identifier, and counts are worded
  through the language's own plural rules, so Russian gets 1 трек / 2 трека /
  5 треков without any call site knowing how many forms a language has. EXPORTER
  is translated in full as the first panel; everything else still reads English
  and will follow.
- **RENAMER, GENERATOR and DEDUPLICATOR speak Russian too.** Their headings,
  hints, controls, tooltips and messages join EXPORTER in the catalogues —
  including the counts in the panel headings, which now agree properly:
  «3 файла», «5 файлов», «21 файл».
- **TAGGER speaks Russian, and so do the field names.** Its three sub-tabs —
  ONLINE, EDITOR, FROM NAME — join the rest, and with them the tag field names
  themselves: the table headers, the column picker, the grouping menu and the
  field editor all read Исполнитель, Название, Альбом, and rename together when
  the language changes.
- **The chrome around the panels is translated too.** The top bar, the mode
  tabs, the table toolbar, the status bar and player, the drop cue, both context
  menus, the dialogs and the whole settings sheet. `plural()` — two English
  forms and no way to ask for a third — is gone from every module that shows a
  count to a person.
- **The interface is Russian end to end.** Cover art, the online source with
  its search and matching report, the columns menu, the rule chain, the player
  and the settings actions were the last of it — 471 keys, English and Russian
  in step. What still answers in English is the text composed in Rust: the
  errors, the placeholder reference and the plan descriptions, filed as its own
  work.
- **What a change was called is translated too, and stays translatable.** A
  plan described itself in English composed in Rust — and that description is
  written into the undo journal, so it would have frozen each row in whichever
  language was active when the batch ran. A plan now carries a code and its
  values beside the English, the interface renders it in the language of the
  day, and the journal keeps both. A batch recorded by an earlier build shows
  the English it was written with; nothing is guessed from the prose.
- **Failures are translated too.** Every message the backend can fail with —
  53 of them, from a mask that will not parse to a folder that is not there —
  now travels as a code and its values instead of a finished English sentence,
  and reads in the chosen language. Where a failure comes from outside the app
  (the operating system, the tag backend, SQLite) the class is translated and
  the detail stays in the words its author wrote, which is what a bug report
  needs.
- **The placeholder reference reads in your language.** The help behind the
  **?** beside every mask field — what each of the 78 placeholders and functions
  does, and the headings they sit under — is translated, and the filter searches
  the translated text. A function keeps its signature, which is grammar rather
  than prose. With this the interface is Russian end to end: nothing a user
  reads is composed in English any more.
- **The interface speaks Ukrainian.** A third catalogue joins English and
  Russian — all 636 keys, from the panel headings to the placeholder reference
  and the failures composed in Rust — and Settings › Display gains
  **Українська** beside them. Auto picks it up from a system set to Ukrainian.
  Counts go through the language's own rules, so it reads 1 трек / 2 треки /
  5 треків. The test that keeps the catalogues in step now reads the catalogue
  directory instead of naming the languages, so a fourth one is checked without
  the test having to be edited.
- **The language control reads Auto · English · Українська · Русский.** The two
  Cyrillic catalogues sat in the order they were written rather than in one a
  reader can see.

- **A CUE sheet is an export format.** EXPORTER offers it beside the playlist,
  CSV, HTML, XML and the mask report: the selection becomes one sheet with a
  `FILE` per track, each starting at `INDEX 01 00:00:00`, which is the shape a
  folder of separate tracks takes. Paths are relative to the sheet, the way the
  playlist's already were. Tracks are numbered by their position rather than by
  their track tag, since the format requires them to ascend and a selection can
  hold gaps or repeats. The `ISRC` line is written only when the field really
  holds one — real files keep catalogue numbers and notes in there, and that
  command takes a bare code, so an unchecked value would spill across the line.
- **One playlist per folder, or per album.** EXPORTER's playlist format gains a
  **One per** choice: leave it at *Selection* for the single list it always
  wrote, or pick *Folder* or *Album* to get one apiece. The file-name field
  becomes a name mask when you do — `%foldername%.m3u`, `%album%.m3u`, or
  anything else the mask language renders — and the files land in the library
  root beside every other export, with entry paths still relative to it. Two
  groups that render the same name get numbered rather than one overwriting the
  other, and a mask that renders to nothing falls back to the folder's name.

### Fixed

- **A message stops repeating the path the window is already showing.** Opening
  a folder raised a toast carrying the whole absolute path — two lines of it,
  three quarters of the width — while the path field above the table had just
  been filled with the same string and keeps showing it. The messages that
  named a path name the folder now, which takes the widest of them from 802 to
  183 pixels and back onto one line. The box is capped rather than free to
  claim 80% of the window, and it runs at the size of the chrome around it
  instead of carrying the largest type in the interface; a long error still
  wraps and stays whole, since a message worth reading is worth its second
  line.

- **An exported playlist survives being copied to another machine.** Entry
  paths were written with the platform's own separator, so a playlist or CUE
  sheet produced on Windows said `Ambient\a.flac` and stopped resolving the
  moment the folder travelled to a Mac or a Linux box — which is the whole
  point of the entry being relative. Every exported relative path is written
  with `/` now, which resolves in both directions. A file name that genuinely
  contains a backslash — legal on Unix — keeps it, since the segments are
  joined rather than the string patched. Absolute paths, written when a track
  lives outside the opened library, keep the platform's spelling: that entry
  does not travel anywhere, and rewriting it would only break it at home too.

## [0.13.0] - 2026-08-22

This release opens the app from outside itself. A folder can be handed to TagRex
by the file manager — Finder's **Open With**, the Dock icon, a folder dropped on
the executable, a second launch — where before it could only be typed in, browsed
to, or dropped on a window that was already up. That needed a file association
the bundle had never declared, and on macOS an Apple Event rather than the
command line this had always been described as.

The correctness fix underneath matters more to anyone whose files carry lyrics:
every save wrote a second `USLT` frame beside the one the file already had, so a
file stated its lyrics twice and typing into the field reached only the copy a
reader might not pick. It writes one frame now, in the language the file used.
The rest is the project's own record catching up — a release page takes its text
from this file instead of going out as a list of downloads, the seventeen that
had already shipped were filled in, and 0.5.0 has a section again after its
heading was renamed away when 0.5.1 was cut.

### Added

- **A folder can be handed to TagRex by the file manager.** Dropping one on the
  open window has worked for a while; starting the app *with* one did not —
  Finder's **Open With**, the Dock icon, a folder dropped on the executable, a
  second launch. All of them now open that folder, through the same path a drop
  takes. macOS does not pass it as an argument, which is what made this look
  like a command-line feature: a double click arrives as an Apple Event, so
  both routes are handled, and the bundle declares the association it needs to
  appear in **Open With** at all. Folders are what TagRex asks to open; for
  audio files it registers as a secondary handler only, so it never takes the
  double-click away from whatever plays them. A second launch on Windows and
  Linux loads its folder into the window that is already open instead of
  starting another copy — on macOS the system already does that.

### Fixed

- **A release page says what changed.** Every release so far was a list of files
  and nothing else: the body was empty on 0.10.0 through 0.12.0, while
  `CHANGELOG.md` carried a written section for each of them. The release
  workflow takes the body from that section — the words it was written in,
  rather than a generated list of commit subjects — and the four that went out
  without one have been filled in.
- **The other thirteen release pages say what changed too.** Filling in the
  bodies stopped at the four newest, so every release from 0.1.0 to 0.9.0 was
  still a list of files while `CHANGELOG.md` held a written section for each.
  All of them now carry that section. 0.5.0 needed its own handling: its
  heading had been renamed rather than superseded when 0.5.1 was cut, so its
  entries live under 0.5.1's heading in the current file and its page was
  filled from the file as it stood at its own tag — leaving 0.5.1's page the
  one fix it actually shipped, instead of the two repeating each other.
- **0.5.0 has a section again.** The file went from `[0.5.1]` straight to
  `[0.4.0]`: when 0.5.1 was cut, 0.5.0's heading was renamed to the new number
  instead of a new section being opened above it, so a whole release — a third
  metadata source, the tempo and label write fixes — was filed under the patch
  that followed it, and 0.5.0 itself had no record at all. The two are split
  back apart, each holding what it shipped. Every version's section and its
  release page now say the same thing. The other twelve tags were checked for
  the same mistake and are clean.
- **A file with lyrics keeps one copy of them, and an edit reaches it.** Lyrics
  have no field of their own — they arrive through the custom-field catch-all,
  under the frame's own name — so the model held the text while the language
  and description that identify an ID3v2 lyrics frame stayed behind. Every save
  wrote a second frame under `XXX`, "undefined", beside the one the file already
  had, and left the original in place: the file stated its lyrics twice, and
  which of the two a reader saw was luck. Typing into the field changed only the
  new copy. The value now lands in the file's own frame, keeping the language it
  was written with, and lyrics in a language the model never read are left where
  they are.

## [0.12.0] - 2026-08-20

This release is about the rule chains. They belong to the job now — importing a
release, reading tags out of a file name, renaming, and the general-purpose one
in GENERATOR each carry their own — and each runs as part of that panel's own
button instead of being a second press to remember. Every rule names the field
it acts on, so one chain can upper-case a catalogue number while title-casing
the titles, and the whole thing is set up in a dialog rather than in a panel
that took a third of the side column.

Two transform steps were fixed against the data they get pointed at rather than
the data they were written for: reading a romanization back into Cyrillic now
judges a value as a whole instead of turning an English title into half-Cyrillic
mush, and the key rule no longer reads *A Minor Detail* as a key and replaces it
with `8A`. The rest is the interface catching up with itself — the preview
player, the app's own tooltips in place of the platform's, and a long tail of
alignment, spacing and focus corrections — and the documentation catching up
with the app.

### Changed

- **The README says what version this is, and what the app can actually do.**
  The status line on the front page read `Status: 0.3.x` while the release was
  0.11.1 — the release checklist names the three files the version has to agree
  in and the README was not one of them, so eight minor versions went past it.
  It now tracks the release, and two claims under **Not yet** that had shipped
  long ago are gone: reading tags back out of a file name, which has been a
  TAGGER sub-tab for many releases, and multi-value fields. The tag block work
  — which containers a file carries, dropping one, converting between kinds and
  between the two ID3v2 revisions — was missing from the page entirely and now
  has a paragraph. The checklist gained the status line, so it moves with the
  version from here on.
- **`docs/architecture.md` describes the program that exists.** It was written
  before any code did and still read that way, so its predictions had gone
  stale: the duplicate finder and the "exotic" formats had come off the deferred
  list, scripting had arrived along the predicted line as saved chains plus a
  mask function language, the provider list was one short, and four of the five
  implementation steps were done. It also gained the two invariants a
  contributor needs before adding anything — a mask calling a function is
  render-only, and every plan returns through the one gate where field locks are
  enforced — plus the cover, export and matching modules, and the frontend and
  audio parts of the stack.

- **The view controls are glyphs, and the app draws its own tooltips.**
  Browse…, Presets, Columns and Groups each carried a word beside their icon
  while every neighbouring control in the same rows — refresh, collapse, undo,
  settings, the transport — was a bare glyph, so the labelled four were the
  widest things in the chrome and pulled the eye away from the button that
  actually changes files. The words are gone from those four only: everything
  that writes to disk keeps its label, because a control that touches files must
  never be a glyph to guess at. **Browse…/Open kept its state without its
  word** — the glyph is a folder while it offers to choose one and the import
  arrow while it offers to open what you typed, and the tooltip says which in
  words. The tooltip itself is new: the platform bubble takes about a second and
  is drawn by the OS, which is too slow and too foreign behind a control that
  has no other label, so this one is the app's own, appears after 400 ms, and
  covers the whole chrome — the bars, the mode tabs and the panels — rather than
  only the four buttons, since two tooltip styles in one row would be worse than
  either. It stays off the table, where the native bubble suits the cells.
- **The table's horizontal scrollbar stopped pretending to be a divider.** It
  runs the width of the window immediately above the status bar, in the same
  colour as that bar's own top rule, and on a table only a little wider than the
  window its thumb spans nearly the whole width — which is why it read as a grey
  strip belonging to the player rather than as a scrollbar. It is now thinner
  than the vertical one (9px against 11), lighter, and lifted off the bar. Still
  always visible: horizontal scroll is not obvious in a wide table, and an
  auto-hiding bar would hide the only hint that there are more columns.

### Fixed

- **The rule chain is set up in a dialog of its own.** Reaching it used to be:
  press the wand → a popover opens over the table → press **Groups** → a second
  popover opens over the first → find the group, press **Load**; and a click
  anywhere outside dismissed the lot. The popover also covered the very rows
  about to change. The wand opens a dialog instead, named for the job whose
  chain it is, with room for a dozen rules and the shelf to load them from; the
  side panel keeps its space for the mode's own work. Leaving that job closes
  the dialog rather than quietly retargeting what is being typed into it. In
  GENERATOR the block stays in the panel that owns it — that mode *is* the
  transform panel and has the room — and there is still exactly one of it, moved
  rather than copied, so the entry points cannot drift. The button carries **a
  dot while the job's chain has rules in it**, and the count in its tooltip:
  the chain runs on its own now, so whether one will is answerable without
  opening anything.
- **The name being parsed is a field of its own.** It sat on the first line of
  the same box as the values read out of it, in the same size and colour, so the
  subject of the operation and its result looked like one list — with the
  longest and least structured line in that list being the one that was not a
  result at all. It now sits above them in its own field, the way the pattern it
  is matched against sits above it.
- **Changing a chain shows its effect at once.** Set a rule, close the dialog,
  and the panel that owns that chain catches up without another press: FROM NAME
  re-reads the line-up under its pattern, and a plan already staged from FROM
  NAME or RENAMER is re-run — RENAMER has no read-out of its own, its staged
  diff is the example. Re-running is exactly what pressing the button again
  would do, which is what you would otherwise have to remember.
- **The FROM NAME read-out shows what the button will actually produce.** The
  live line-up under the pattern showed the raw extraction — `the_x_factor`,
  `desert_rain` — while the chain that turns underscores into spaces and
  title-cases them sat right there waiting to run. That was fine when the chain
  was a separate press; since it runs as part of **Preview tags**, the read-out
  was previewing values that would never be written. It now goes through the
  same chain, by the same path the real plan takes, so what is under the pattern
  is what lands in the table.
- **Key notation stopped eating titles.** The mode after a note letter was
  matched with `starts_with`, so anything beginning with *min* or *maj* counted
  as a whole key: **A Minor Detail** became `8A` and **A Major Reason** became
  `11B`. Harmless while the rule only ever ran on the Key field, and not
  harmless now that a chain can be aimed at every tag field. The mode must be
  one of the spellings the step actually models — nothing, `m`, `min`, `minor`,
  `maj`, `major` — and anything else means this is not a key. `Am`, `A min`,
  `A-minor` and `A major` are unaffected. The other rules were swept for the
  same shape and are clean: removing diacritics works off an explicit Latin
  table, so Cyrillic `Ё` and `Й` keep their marks; transliteration produces
  Latin from Cyrillic and cannot mix scripts; the case exceptions are a curated
  list that deliberately leaves out single-letter roman numerals.
- **Transliterate to Cyrillic decides once per value.** Requiring a trace of
  romanization left exactly one word converted in an English title —
  `la_буш_-_music_from_the_temple_of_house` — because `sh` is both an English
  digraph and the romanization of ш, and nothing *in that word* can say which.
  The value can: `music` and `house` were never Cyrillic, so the line is Latin
  that was always Latin. Now the whole value converts or none of it does, which
  also means a real romanization comes back whole — `Masha i Medved` converts
  including the words with no trace of their own, where before they would have
  stayed Latin. The cost, the same answer from the other side: a value mixing
  the two languages (`Zhuk remix`) is left whole rather than half-converted.
- **Transliterate to Cyrillic stopped mangling English text.** Run over tags
  read from an English file name it turned `desert rain` into `десерт раин` and
  left `music` and `house` in Latin — every bit of it the documented behaviour,
  because the rule asked only whether a word *could* be read back and most short
  English words can. It now asks whether the word *looks* romanized, by the
  traces the forward direction leaves for sounds Latin has no letter for — `zh
  kh ts ch sh shch yu ya yo iy yy` — so English text is left alone. The cost is
  stated rather than hidden: a romanized word made only of plain letters (`dom`,
  `Kino`, `na`) has nothing to recognise it by and is left alone too.
  Under-converting can be fixed by hand; a mangled library cannot.
- **A chain can be emptied in one go, and its kind picker stopped stretching.**
  Starting over meant one × per rule, each re-rendering and renumbering what was
  left; **Clear rules** sits beside Add rule and appears only when there is
  something to clear. The picker beside it was `flex: 1` and ran the width of
  the block for an option needing a third of it — measured at 156px sized to its
  longest kind, against a 560px row.
- **Two texts that outlived what they described.** The FROM NAME help ended with
  "tidy them up with **Clean up** on the preview bar" — a button that no longer
  exists, for a step that is no longer separate — and the wand's tooltip in the
  markup still said it pinned the rules to the side panel. Every other hint and
  all 47 tooltips were checked against what is on screen and name something that
  exists.
- **Three places stopped saying Discogs when the source is something else.**
  The empty state under the results read *"Search Discogs to see releases."*
  with MusicBrainz selected two rows above it; the TAGGER tab's tooltip named
  one of the three sources; and a staged import was described as `Import Discogs
  release` whatever it came from — which put the wrong source in the plan bar
  and in the undo journal, where the description is what tells you months later
  what a batch did. The first two are now generic, and the plan names the source
  it actually used. Where the provider *is* the subject — its token field, the
  shipped `Discogs cleanup` preset, the `Discogs Release ID` frame — the name
  stays.
- **The table's placeholder has room again.** Lining that box up with its
  neighbours took its margin away everywhere — right in the rule chain, where it
  stands among full-width controls, wrong in the file table, where it stands
  alone in an area and landed flat against three borders. It keeps its distance
  where it stands alone and takes the rows' width where it stands among them.
  Its corner matches them too: 4px among 6px cards was the one outlier in a
  sweep of every border in the app — 6px for controls, 4px for the two small
  filter toggles, 10px for large surfaces, 999px for the floating plan pill and
  partial radii for the fused halves of the search field, all as intended.
- **The empty-state box lines up with the rows around it.** It carried a margin
  of its own, so the dashed placeholder stood a few pixels narrower than the
  controls above and below it — a misalignment where a stand-in for those very
  controls should match them. Its corners match them now too, instead of being
  the one rounder thing in the column.
- **Empty states lost their diagonal stripes.** The inert motif behind one line
  of text was busier than the sentence in front of it, and heavier again in the
  dark theme; the dashed edge says "placeholder" on its own. The stripes stay
  where the texture earns its keep — the cover well, disabled controls, and the
  skeleton loaders, where the movement is the point.
- **Empty states are a line, not a panel — everywhere.** "Nothing here yet" had
  24px of padding inside a striped box with another 10px of margin around it, in
  the file table, the search results, the deduplicator and the rule chain alike:
  the least valuable sentence on screen, given the most room on it. It gets a
  line's worth now.
- **The folder path takes focus as one control.** Sweeping for the shape behind
  the search-field fix turned up the same arrangement in the top bar: the button
  naming the open folder and the caret beside it are borderless halves of a box
  that carries the border and the fill, so the shared focus ring drew itself
  around whichever half had focus, inside the outline that is already the field.
  The box takes the ring and the accent edge now, and its halves stop drawing
  their own. The filter's two flag toggles and the List/Grid segments were
  checked and left alone: those are separate controls sitting together, not
  halves of one.
- **The focus edge stopped leaking inside the search field.** The Discogs query
  is a composite — an input and the caret that opens recent queries, sharing one
  outline — and it draws focus as a whole. The accent edge added with the softer
  ring belonged to whichever half had focus, so it showed through as a green
  sliver one line inside the ring already marking the field: two indicators for
  one control, one of them a fragment. Both halves take the edge now, so the
  pair reads as the single field it is.
- **The scope select is as wide as what it holds.** `Apply to` stretched to the
  full width of the panel while its longest option is *All tag fields* — 505px
  of control for a 110px value, with the menu it opens sized to its own content
  and so visibly narrower than the thing that opened it. And the focus ring it
  wore is softer: two solid pixels of accent around a control that already has a
  border read as a slab, so the halo is translucent — it keeps its hue over a
  panel, a row or the table, in both themes — while the control's own border
  goes full accent, which is what makes the focus unmistakable.
- **The transform dialog spends its height on the chain.** It opened with two
  headings for one thing — the dialog's, naming the job, and the block's own
  saying less — followed by a general line about previews that the line under it
  already answered precisely. An empty chain then showed a 120px striped panel
  to say *nothing here yet*: the least valuable sentence on screen given the
  most room on it. The second heading and the redundant hint are gone, "no rules
  yet" is one quiet line, centred as it was, and the dialog grows with the chain
  up to 92% of the window — a cap, not a height, so a chain of two rules does not
  come with half a screen of nothing under it. The group shelf can grow to 240px
  inside it, and the shelf of groups has **no cap of its own** in there: a second,
  smaller limit inside the dialog made the list you pick from a four-row
  scrolling window while the dialog had room to spare, so the eight built-in
  groups now stand at full height and the dialog grows to fit them (#244). If it
  ever reaches the window, the body scrolls as one column instead of two nested
  scroll areas. The seams inside it are
  tight — one form read top to bottom does not need the spacing of a page with
  regions to tell apart — and the note about when the chain runs is a line of
  text rather than a tinted box.
- **The chain-level scope row is gone.** With the target on every rule it had
  one job left — setting what the next rule starts on — which is a labelled row
  for a default you can change in the rule's own row a second later. A new rule
  inherits from the rule above it instead, so a run of steps over one field
  still costs one choice. Loading a saved group now writes its group-level
  target onto each of its rules, since there is nowhere else to keep it, and a
  scope this build doesn't list stays on the rule rather than being silently
  re-aimed at every tag.
- **Each rule aims at its own field.** The scope was a property of the whole
  chain, so one chain had one target and "upper-case the catalogue number, lower
  the titles" meant two chains — with the second one a saved group to remember
  to run by hand. The target now belongs to the rule: every rule row picks one,
  and the row above the chain is what a new rule starts on. Consecutive rules
  that agree run as one chain, so a chain whose rules all name the same thing is
  exactly what it was before, order preserved either way. Saved groups can hold
  it too — the scope on a rule is optional and absent means "whatever the group
  says", which is every group written until now. And the picker offers **every
  modeled field** rather than ten of them: Catalogue #, BPM, Composer,
  Publisher, ISRC, URL, Year, Track and the rest were always understood by the
  runner, just never listed.
- **A rule chain per job, and it runs as part of the job.** One chain shared by
  everything was wrong in a way that only shows in use: RENAMER wants a space
  turned into an underscore and FROM NAME wants exactly the opposite, so
  whichever was set last quietly ruined the other. Four jobs now have a chain of
  their own — importing a release, reading tags out of a name, renaming, and the
  general-purpose one in GENERATOR — and each persists, so it is set up once.
  **EDITOR has none on purpose**: a value typed by hand must come out as typed,
  alternating capitals and all, and the wand is not offered there or in the
  modes that produce no values. The saved groups stay global, because a shelf
  you load from should not depend on where you stand.
- **One press instead of two.** Getting cleaned-up tags out of a file name meant
  pressing **Preview tags** and then remembering to press **Clean up staged** —
  and forgetting the second gave you the raw values with no error to say so. The
  context's own action now runs its own chain over the plan it just built: the
  tags then FROM NAME's rules, the new names then RENAMER's, the provider's
  values then ONLINE's *before* they reach the edit buffer — which is where the
  shipped **Discogs cleanup** group was always meant to be used. One press, one
  Apply, one undo entry; an empty chain changes nothing. The docked block lost
  its run button and the plan bar lost **Clean up**, both of which existed only
  for the second press. A plan staged from somewhere with no chain of its own —
  a hand edit, a cover change — is still cleaned in GENERATOR, whose button
  retargets to a staged plan when there is one. A failing chain gives back the
  plan it was handed rather than nothing.
- **The rule groups are a list in the block, not a popover over a popover.**
  Every row carried three controls doing similar things — a tick, a name that
  toggled the tick, and a **Load** link. Clicking the name now loads the group
  into the chain, the link is gone, and the ticks stay for what they are for:
  composing several groups into one plan with **Run N ticked**. Saving and
  deleting are where they were.
- **The staged-plan bar stays inside the window, and stops reciting the mask.**
  The pill is centred on the table and was as wide as its contents demanded, so
  in a narrow window half of it hung off the left edge with no way to bring it
  back. It is now capped to the table's width, and what gives way is the plan
  description — the count, the Show-old toggle and the three gate buttons keep
  their full size at any width. The description itself was the whole pattern:
  `Tags from name: %albumartist%_-_%album%_(%catalognumber%)…`, the longest
  thing on screen saying the least, a few centimetres under the field where that
  pattern had just been typed. The bar now names the operation — *Tags from
  name*, *Rename by mask* — with the full text on hover. The undo journal still
  records the whole description, which is where the mask is what tells you
  months later what a batch did.
- **The group list stopped collapsing into an unlabelled grey strip.** In a
  flex column an item that scrolls may shrink below its content, and short of
  room this one did — down to ten pixels of padding with its rows still inside,
  showing as a grey bar with nothing on it. It keeps its height now and the
  block around it scrolls.
- **The tooltip wraps instead of eating its own text.** It went out as one line
  with an ellipsis and a 260px cap, so every title longer than that was cut in
  half — "Transform the selected files' tags — the GENER…". On a control whose
  only label is its glyph the tooltip is the whole explanation, and the half
  that went was the half saying what the button does. Up to 280px and as many
  lines as the sentence needs; nothing sits below a tooltip to be pushed out of
  place.
- **The Play button in the status bar is flat like the rest of the player.** It
  is the player's own toggle while the row is down, but it sits outside the
  player element, so the rule that flattened the transport buttons never reached
  it: a bordered, filled, rounded box on a bar of flat glyphs and small text.
  Same size, same glyph, same hover fill as the transport now.
- **The hairline under the player's track title is gone.** It was never a border
  or a divider: the shared button style ends with a small drop shadow, and every
  rule that flattens a button stripped the border and the fill but not the
  shadow — on a transparent, borderless box the only thing left for it to paint
  is a 1px smudge along the bottom edge. The title box spans the whole row,
  which is why it read as a line drawn from the title to just short of the time.
  Sweeping the live interface for the same combination found 25 of them: the
  folder path controls in the top bar, every padlock in the editor, and every
  entry of both context menus. All reset, the way the icon buttons and the small
  text buttons already were.

## [0.11.1] - 2026-08-20

### Fixed

- **The Windows build works again.** Asking for the trash crate without its
  default features — done to drop a timestamp reader nothing here uses — also
  dropped the COM apartment mode, which is not optional: on Windows that crate
  refuses to compile unless one is named. Nothing showed on macOS or Linux,
  where the module is not built, and CI builds only Linux, so the first sign of
  it was the 0.11.0 release build failing on both Windows targets. The apartment
  mode is now asked for by name.
- **CI compiles for Windows now.** The break above was a dependency wrong for
  one platform only, and nothing looked at that platform until a tag was pushed
  — which is after a release is published. A second job runs clippy and the
  tests on a Windows runner on every push. Not a bundle build: the release
  workflow packages the installers, this only has to prove the code compiles.
- **Four tests no longer assume a POSIX path separator.** The first Windows run
  found them: three built an expected path as one `join` of `"folder/name.flac"`,
  where the `/` stays literal on Windows while the code under test joins
  components and produces `\`, and one took a file name off a path by splitting
  on `/`. The expectations were wrong, not the behaviour — the pattern that has
  to accept either separator passed. Test-only; nothing shipped changes.

## [0.11.0] - 2026-08-20

### Added

- **The seek bar draws the track's waveform.** The plain slider is now a
  loudness envelope, with the played part in the accent colour, a playhead, and
  the same click-and-drag seeking it always had — the range input is still the
  control underneath, so the keyboard and every existing behaviour are
  unchanged. The envelope is **RMS, not peak**: the loudest sample per bucket
  draws a solid block for anything mastered in the last thirty years (measured
  on a real track it sat at 216 of 255 with most buckets at the ceiling), while
  RMS follows how loud a passage actually is, so an intro, a breakdown and a
  drop are three different heights. Decoded through the same path playback uses,
  so anything the player can play is something the bar can draw, and cached per
  file and modification time so replaying a track costs nothing. A track being
  decoded shows a centre line and still seeks; a file that will not decode keeps
  it, silently.
- **A button to re-read the open folder.** Files change under the app — one is
  dropped into the folder, a track is edited elsewhere — and the only way to see
  that was to go back through the folder chooser. The button beside Browse…
  re-scans and re-reads tags while keeping everything a reopen would throw away:
  sort, grouping, filter, columns, the mode you are in, the selection and any
  pending edits, all of which survive for files that are still there and are
  dropped for files that are not. It reports what moved ("3 tracks, 1 more"),
  and it is inert while a change is staged — discarding a plan is a decision,
  not a side effect of refreshing.
- **A right-click menu on the file column**, with two entries. **Remove from the
  list** takes files out of the table without touching disk, for narrowing a
  working set by hand; a re-read brings them back. **Move to Trash…** puts them
  in the system Trash after a confirmation that names how many. The menu acts on
  the whole selection when the row you clicked is part of it, and otherwise on
  that one row — which it selects first, so what is about to happen is visible
  before it happens. A deletion is confined to the open folder the way every
  write already is, refuses the whole batch if any path fails that check, and is
  deliberately not in the undo history: the journal cannot bring a file back out
  of the Trash, which is exactly why this is the Trash and not a delete.
- **Fields can be locked against change.** A padlock beside every field in the
  EDITOR panel marks it as not-to-be-touched: no import, transform, rename,
  clear-tags or hand edit can change a locked field, its column in the table
  carries a padlock in the header and its cells no longer open for editing. The
  lock is enforced where plans are built, not where they are applied — a locked
  field never enters a change in the first place, so the diff you approve is
  the diff that runs. What a lock kept out is reported rather than silently
  dropped: the Apply bar names the fields and how many files each would have
  touched, and an operation a lock leaves with nothing to do says so instead of
  reading as "nothing to change". Locks last for the session and are
  deliberately not persisted — one set months ago and long forgotten would make
  every operation quietly do less than it says. The Track/Disc numbers lock as
  one, since a renumbering rewrites them together.

### Changed

- **The README describes masks as they now are.** Its RENAMER paragraph still
  presented a mask as placeholders, `[...]` sections and zero-padding — the
  grammar before the function language landed. It now says what a mask actually
  is: `$name(arg,arg)` calls around the placeholders, arguments that are
  patterns themselves and therefore nest, forty-one functions across the three
  groups, the empty-is-false rule the language shares with `[...]`, and why a
  mask that calls a function only renders. Discoverability for someone reading
  the repo; the in-app reference already listed every function.

- **Dependencies are compiled optimised in debug builds too.** Only
  dependencies — our own crates stay unoptimised and quick to rebuild. Audio
  decoding is why: a waveform for a six-minute track took 26 seconds through an
  unoptimised decoder against half a second through an optimised one, and the
  debug bundle is what gets run all day. The cost is one slower first build,
  since dependencies are compiled once.


### Fixed

- **The file count no longer wraps to make room for the player.** In a narrow
  window "3799 files · 3 selected" broke across two lines and took the whole bar
  taller with it. The label is a short fixed phrase and now neither shrinks nor
  wraps; the player gives way instead, which its waveform can afford — it just
  shows less detail.
- **One bar at the bottom instead of two.** The status bar and the player were
  two strips stacked on each other — about 90px of chrome for what reads as one
  — with the whole middle of the status bar empty. The player now sits in that
  middle: the file count on the left, the player between, Preview edits on the
  right, and the bar shrinks back when nothing is playing.
- **Clicking the player's title finds that track in the table.** It selects the
  track alone, makes it the keyboard anchor and scrolls it into view — after a
  few minutes of listening, with a filter on or the table scrolled elsewhere,
  "which row is this?" is a real question and the bar is the only thing that
  knows. A file no longer in the open library says so.
- **The player row gives its width to the waveform.** The elapsed/total time
  moved onto the title's line, at the right end of it — the two answer the same
  question, and the time no longer needs a column beside the bar. Volume folded
  away behind its own button: a slider used for a few seconds at a time was
  holding a fixed strip of the row permanently, and the button still shows the
  state, so muted is readable without opening anything. Clicking it opens a
  popover with the mute toggle beside the slider. Between them the waveform
  gained a few hundred pixels.
- **The unplayed half of the waveform can be seen.** It was drawn in the border
  colour — a hairline colour by definition, wrong for half the picture — and is
  now the muted text colour at half opacity, legible in both themes without
  competing with the played part.
- **The played colour no longer lags the playhead.** A bar was painted as played
  only once all of it was behind the cursor, so the bar under the cursor stayed
  grey and the colour appeared to trail it. A bar counts as played once the
  playhead has entered it.
- **A thinner volume slider**, and **the player row no longer selects as text** —
  dragging the seek bar used to highlight the title and the clock along the way.
  (Fixed twice: the first attempt carried only the unprefixed `user-select`,
  which a Chromium check passes and the app's own webview does not.)
- **The player's transport buttons are flat, and the track title is readable.**
  The five glyphs wore the shared button chrome — a border, a fill and a shadow
  each, and not even the same width, since the repeat button carries a
  superscript — so a row of transport controls read as a row of boxes. They are
  glyph-only equal squares now, with a fill only under the pointer. The title
  moved from a fixed column beside the seek bar, where most titles were
  ellipsised after a few words, to its own line above the waveform, where it has
  the whole width; the seek bar's length no longer depends on it either, which
  is what the fixed column was for. Disabled transport buttons dim again — the
  waveform change had rewritten the selector that did it.
- **The field padlocks no longer come and go with the mouse.** They were
  revealed on row hover, and WebKit holds a stale hover state on rows the
  pointer has merely passed over — so a random handful stayed on screen and the
  rest did not. They are always visible now: a faint outline beside every field,
  accent-coloured once the field is locked.
- **Locking a column no longer makes the table header a line taller.** The
  padlock in the header wrapped onto a line of its own — the shared icon glyph
  is a block element, and the header label does not wrap — so the whole header
  row grew the moment any field was locked. It now sits beside the column name
  and the header keeps its height.

- **A cell being edited offers the values the library already holds.** Typing an
  album artist that eleven other files already carry is how a single letter of
  drift gets in — "Various" beside "various", "Warp Records" beside "Warp" — so
  a double-clicked cell now lists what the open files say for that column, with
  the count of how many carry each value beside it, most-used first. What starts
  with the typed text comes before what merely contains it, and the matching
  part is marked. A value staged but not yet applied counts too: correcting one
  row and reaching for the same wording on the next is the case this is for.
  **It never types for you** — nothing is inserted until a row is chosen, no row
  is highlighted by default, and Enter reaches the list only after ↓ or ↑ has
  stepped into it, so "type it and press Enter" is exactly the gesture it always
  was. ↓ on an untouched cell opens the whole list, Esc dismisses it, and a
  click picks a row. Silent on the columns where a repeat means nothing — title,
  ISRC, URL, track/disc numbers and BPM.

- **Masks can do arithmetic.** The third and last group of the mask function
  library: `$add` `$sub` `$mul` `$div` `$mod` `$min` `$max` `$round`, listed
  under **Math** in the reference popover. Numbers are decimal, because the one
  field anybody really computes with is BPM and it is routinely `128.5` —
  `$div(%bpm%,2)` on a 128.5 track gives `64.25`, not a truncated `64`, and a
  result never comes out as `64.00000001`: six decimals, trailing zeros
  removed. Two decisions hold across the whole group. An operand that does not
  read as a number counts as **0**, so `$add(%bpm%,1)` on a file with no tempo
  is a `1` rather than a rename of a thousand files stopped by one of them; the
  places argument of `$round` is written in the pattern, so a bad one still
  reports itself. Division by zero produces **nothing** — the language's own
  empty value, which a `[…]` section drops around and `$if2` can replace. Note
  that `0` is a value, so a section wrapped around a computed value always
  survives: `[' ('$if(%bpm%,$div(%bpm%,2))')']` is how a pattern says "only when
  there is a tempo".

## [0.10.0] - 2026-08-18

### Added

- **An import can bring the release's cover with it.** Until now importing tags
  from a release wrote the text fields and nothing else; getting the artwork on
  took a second button, a second Apply and — the part that actually bit — a
  second entry in the journal, so undoing "the import" gave back the tags and
  left the cover behind. Now they are one change: one Apply writes both, one
  undo takes back both. **Settings › Online import › Cover art** has three
  states, and the default is **If missing** — a file carrying no artwork gets
  the release cover, a file that already has one is left exactly as it is,
  decided per file so a mixed selection is filled in precisely where it is
  needed. **Always** replaces existing artwork; **Never** is the old behaviour.
  Because the table has no column for artwork and a cover change would otherwise
  make a row go quietly staged, the action bar and the toast now name it —
  "Edit tags + cover on 3 files". (#207)

### Fixed

- **Undo now brings a destroyed ID3v2 block back frame for frame.** Removing or
  converting a block used to put it back by rebuilding it from what the app can
  read — its fields and its pictures — so anything the app has no field for was
  gone for good: DJ cue points, ratings, player-specific frames. The app warned
  about it, but a warning is not an undo, and the reachable case was real —
  converting away from ID3v2 destroys the block that holds exactly that data.
  The block is now kept as bytes before it is destroyed, and undo writes those
  bytes back, so what comes out is what went in. ID3v2 only, which is where that
  data lives on the containers people tag; every other kind still rebuilds, and
  still says so before you commit to it. The conversion dialog now tells the two
  apart instead of always assuming the worse one. (#206)

## [0.9.0] - 2026-08-18

### Added

- **A file says which tag blocks it carries.** A new **Tag types** column, and a
  line in the tag editor when there is something to say — `Reading ID3v2 — this
  file also carries ID3v1`. One file can hold several answers to the same
  question, and until now nothing showed you that: the values you saw came from
  one block while other software read another, which is exactly the confusion
  behind #194. The block being read is named first, and it is also the one a
  write goes to. Free — it comes off the probe the listing already does. (#47)

- **And now you can take one of those blocks off.** Beside that line in the tag
  editor there is a **Remove ID3v1** — or APE, or whichever spare block the
  selection carries — and it strips exactly that one, leaving the block the
  values come from, and every other block, untouched. This is the answer to a
  file that shows one artist here and a different one elsewhere: the stale
  second answer goes, the good one stays. It is an ordinary staged change, so
  the diff bar and undo work on it like anything else, and undo puts the block
  back with what it held. Only the spare blocks are offered — emptying the one
  being read is what **Clear tags** is for. One caveat, and the app says it
  before it stages anything: undo rebuilds a block from its text and pictures
  rather than from its bytes, so for anything but ID3v1 a frame the app has no
  field for — a cue point, a rating — would not come back. ID3v1 holds seven
  text fields and nothing else, which is why that one is exact. (#47)

- **Tags can be written as a different kind of tag block.** **Convert…** beside
  the block line in the tag editor: pick what to write these tags as — ID3v2,
  APE, Vorbis Comments, ID3v1, whichever the file's container can carry — and
  the app writes that block from what the current one holds and drops the old
  one, so the file is never left carrying two answers to the same question. Only
  targets *every* selected file can take are offered, and a selection reading
  from two different blocks is not offered the conversion at all, because there
  would be no single source to convert. Undoable like anything else. The one
  case worth naming: **switching an ID3v2 block between 2.3 and 2.4**, which
  restamps the header and keeps every frame — cue points included — rather than
  rebuilding the block, so it is lossless in both directions and undo puts the
  original revision back. Every other conversion rebuilds the block from the
  values the app can read, and says so first: the dialog names the fields the
  target has no room for (worked out by putting the values through the real
  conversion, not from a table) and warns that anything the app cannot read
  would not come across. (#205)

- **A mask can ask a question, not just reshape an answer.** Eleven more
  functions: `$if`, `$if2`, `$equal`, `$nequal`, `$and`, `$or`, `$not`,
  `$greater`, `$longer`, `$isnumber` and `$in`, listed under **Logic** in the
  reference behind every mask box. The one worth reaching for first is
  `$if2(%albumartist%,%artist%)` — the album artist, or the artist when there
  isn't one — which no pattern could express before: an optional `[…]` section
  can drop a part but cannot put another one in its place. There is one rule for
  what counts as true, and it is the rule `[…]` already followed: **a value is
  true when it is not empty**. Inside these functions a missing tag is simply
  empty rather than an error, because asking whether something is there cannot
  fail on the answer being no. `$greater` compares as numbers rather than as
  text, so `9` is not greater than `10`. (#202)

### Fixed

- **Two kinds of awkward file are no longer given up on.** A file that failed to
  read was listed as unreadable and left alone, which was the safe thing to do
  but not always the right one. Reading now tries harder before it gives up: the
  same parse as before, then the backend's other parsing mode — the two forgive
  *different* faults, and a file whose only problem is a malformed identifier
  frame is read by the second — and finally the container the file's extension
  names, for a file whose audio starts far enough behind padding that the format
  is guessed wrong entirely. Found on a real 3799-file library where exactly two
  files were affected: one lost nothing but its own tags, the other was hiding a
  title, a musical key and a tempo. Costs nothing for a file that reads first
  time, which is very nearly all of them. (#204)

## [0.8.0] - 2026-08-17

### Added

- **A mask can call functions.** `$upper(%artist%)`, `$left(%title%,20)`,
  `$swapprefix(%albumartist%)` — twenty-two of them, covering case, slicing,
  trimming, padding, search and replace, and three that know something about
  music: `$stripprefix` and `$swapprefix` handle a leading *The*, and `$cutmix`
  drops a trailing *(Original Mix)* or *(Remastered)* while leaving a remixer
  credit alone, because that one names a different recording. Arguments are
  patterns themselves, so calls nest and a placeholder or an optional `[…]`
  section can sit inside one. The full list is in the reference behind every
  mask box, under **Functions**; clicking one drops an empty call in with the
  cursor already between the brackets. Two things worth knowing: a pattern that
  calls a function can build a name but can no longer *read* tags out of one —
  a transformation cannot be run backwards — and a `$` that doesn't begin a call
  is still just a dollar sign, so patterns written before this keep working.
  (#73)

## [0.7.0] - 2026-08-17

### Changed

- **The tag backend is three releases newer.** Moving from 0.22 to 0.25 brings
  three releases of parsing fixes and format support, and takes away the one
  thing the custom fields were built on: the backend's generic tag no longer has
  any way to hold an item it doesn't recognise. Those items — everything you add
  by hand, plus the ReplayGain values and release ids a real file carries — are
  now read from and written to the format's own tag directly: a `TXXX` frame on
  MP3, AAC, AIFF and WAV, a comment on FLAC and Ogg, a freeform atom on M4A, an
  item on the APE formats. Verified on real files of every one of those: an
  edit, a clear and an undo leave DJ cue points, artwork, ReplayGain, the legacy
  ID3v1 block and everything another tagger wrote exactly where they were. The
  minimum Rust version rises to 1.89 with it. (#201)

### Fixed

- **A custom field keeps the name the file spells it with.** A field the backend
  recognises but the app has no column for — the ReplayGain values, the
  MusicBrainz ids — was listed under an internal name and, worse, written back
  under that name: a `REPLAYGAIN_TRACK_GAIN` read as `ReplayGainTrackGain` and
  the next save put the value in a frame of that name, where nothing else looks
  for it. The name now comes from the file, both ways. (#201)

- **What encoded a file is left alone rather than edited or cleared.** The
  encoder and its settings, the length in milliseconds and the file-type frame
  read back as ordinary editable fields, so they filled rows in the tag editor
  that mean nothing to change — and a clear wiped them along with the metadata.
  They describe how the file was made, not what is on it, so they now sit with
  the encoder header and the DJ cue points: never shown, never written over,
  never claimed as cleared. (#197)

## [0.6.1] - 2026-08-17

### Fixed

- **Clearing the tags on an MP3 no longer leaves a stale ID3v1 tag behind.**
  Clearing looked like it did nothing: the values came back, cut off at thirty
  characters. That is an ID3v1 field — the file carried a legacy tag beside its
  ID3v2 one, only the ID3v2 one was ever written, and once it was cleared the old
  ID3v1 was all that was left to read. The same silence covered ordinary edits:
  the app showed the new value while anything reading ID3v1, older DJ hardware
  included, still read the old one. A file that has one now gets it written with
  everything else, and loses it when there is nothing left to put in it; a file
  without one never acquires one. Cover art and DJ cue points are untouched —
  ID3v1 can hold neither. (#194)

- **A toast no longer covers the staged-change bar.** The message announcing a
  staged change landed on the Discard and Apply buttons it was about; it now sits
  above the bar while one is floating over the table. (#195)

- **Folders keep an order you can predict, and an apply keeps the sort.** With a
  column sort active, folder headers came out in the order of each folder's
  first file — which reads as random — and applying anything re-read the library
  into scan order while the header still showed a sort, so everything jumped and
  the folder being worked on appeared to move. Folders now take their own order
  under a sort, in its direction, and keep the scan order when nothing is
  sorted; an apply or an undo re-sorts what it re-read. (#196)

- **Lining up files with a release no longer moves every folder in the
  library.** Matching against a release, or dragging one row to a new place,
  ended by switching the column sort off — and the sort is what held the folders
  in place, so ordering three files sent every folder somewhere else, and the
  next apply moved them again. Both now keep the sort and say what they did: the
  sorted column's arrow fades to mean "some files were placed by hand", and the
  next sort, apply or undo puts everything back under the header's rule. (#198)

- **Auto-match and Match by length alternate.** The button offering the second
  rule never offered the first one back, so a length match that crossed the
  wrong two files could only be undone by changing the selection. Each press now
  runs its rule and offers the other one. (#199)

- **The search box's focus ring goes round the whole box.** The field and the
  caret beside it are one control, but the ring was drawn round the field alone:
  square where the two meet, spilling over the caret's edge, with the caret's
  grey border carrying on outside it. It now follows the control's own contour,
  from either side. (#200)

## [0.6.0] - 2026-08-17

### Added

- **A release card says how far each track's length is from your file's.** An
  expanded tracklist now reads `8:04 · +2s`, coloured by how far off it is:
  within a couple of seconds is the same recording, up to ten a different master
  or fade, beyond that something else. Each track takes the selected file
  closest to it in length — one file to one track, closest pairs first — rather
  than the position the import would use, so three files of a five-track release
  still line up with the three tracks they really are. A tally in the tracklist
  header ("3 of 5 lengths match") answers the question the rows imply, and both
  follow the selection as it changes. That is enough to tell a rip of one
  edition filed under another's catalogue number apart before anything is
  written. (#188)

- **The last eight folders you opened are one click away.** A caret at the right
  of the library indicator drops the list; picking one opens it. Each row reads
  like the indicator — folder name, parent dimmed in front, whole path in the
  tooltip — most recent first, and reopening a folder moves it up rather than
  adding a second row. A folder that has since moved or been unmounted leaves
  the list when opening it fails, with a word saying so. Loose files dropped on
  the window aren't a folder anyone chose, so they aren't remembered. (#180)

### Changed

- **The top bar names the open folder instead of half-showing its path.** The
  text box that answered "which library is open?" with
  `/Users/me/Music/Temp mus…` is now an indicator: the folder in full, the
  folder above it dimmed in front, the whole path in the tooltip. Click it and
  it becomes the field again, with the path selected so a pasted one replaces
  it; Escape leaves without changing anything.

  Browse… and Open are one button. It offers **Browse…** until the field says
  something other than what is open, and **Open** from then on — decided by that
  difference rather than by focus, so reaching for the button after pasting a
  path doesn't turn it back into Browse just in time to open a folder dialog
  instead. (#177)

### Added

- **A second press on Auto-match orders by length instead of by name.** The
  button matches on names as before and then offers **Match by length**, which
  reorders the selected files to the durations the release lists and ignores
  what they are called. It pairs one file to one track, closest pairs first, and
  it may cross — which is the case names cannot express: two tracks whose names
  are swapped against their lengths, each matching its namesake exactly while
  the running times say otherwise. It claims nothing more than ten seconds out,
  and where two tracks run within a couple of seconds of each other it says the
  order can't be told apart rather than guessing. The offer resets when the
  selection or the search results change; pressing again re-runs the length
  match rather than going back to names. (#191)

### Changed

- **The queries a selection can build hang off the search field itself.** The
  dropdown that named where a query could come from — Manual, Folder name, File
  name, Album, Artist + Title — is gone; a caret at the end of the field opens
  the same sources showing the **text** each would search for, with its name as
  a hint. So you pick a query rather than a mode, and see what it is before
  committing to it. A source with nothing to offer doesn't appear, two sources
  producing the same text are one row wearing both labels, and with nothing
  selected the menu says so. Typing no longer resets a label that was only ever
  a claim about the box. Same shape as the library indicator and its recent
  folders, one control instead of two, and a wider field in a narrow panel.
  (#193)

- **The file table builds only the rows the window shows.** It used to keep one
  DOM row per file, so a library of a few thousand paid for rows nobody could
  see — and every operation was priced against the library rather than against
  the window. What the table shows is now a model, of which only the visible
  slice exists as rows, held in place by spacers so the scrollbar still means
  what it says. Selection, select-all, Shift-ranges, folder selection, keyboard
  navigation and the player's running order read that model, so a file the
  window doesn't hold is still in scope for everything it was in scope for
  before, and fitting a column to its content still measures the whole list —
  the widest values are lent to the table for the measurement. Measured on a
  library of 3799 files, with the DOM down from 4117 rows to about 46: opening
  it 2874 ms → 566 ms, sorting a column 5149 ms → 79 ms, expanding every folder
  2684 ms → 24 ms, deselecting everything 582 ms → 8 ms, a keystroke in the
  filter 56 ms → 2 ms, leaving a staged change 220 ms → 83 ms. (#189)

### Fixed

- **A match by name no longer sounds sure when the lengths disagree.** Auto-match
  decides on titles and never looked at how long anything runs, so a folder
  whose names line up perfectly against running times that say otherwise was
  reported as a clean match. It now weighs the result against the lengths and
  says so: how many disagree and by how much, in the warning colour rather than
  the ordinary one. When a different assignment of the same files would fit the
  lengths markedly better — the crossed case — it points at the second press
  that would do it. Ordinary rounding and fades stay quiet: it takes more than
  ten seconds to be worth a word. (#192)

- **The seek bar seeks.** Dragging it moved the time and the thumb while the
  audio carried on where it was — on FLAC, ALAC, AIFF and WAV; MP3 was fine.
  The audio backend decoded those formats through a decoder with no seek support
  at all, and the player moved its clock without looking at whether the seek had
  been accepted, so the readout told a story nothing was playing. Every format
  now decodes through Symphonia, which knows how to seek, and the clock only
  moves when the audio did — if a decoder ever refuses, the bar goes back to
  where the sound really is and says so instead of pretending. Verified by
  seeking to three seconds before the end of a real file in each format and
  watching playback actually end. (#190)

- **Staging a change no longer rebuilds every row of the library.** Bringing a
  release into three files of a few thousand rebuilt all of them so that three
  could show a diff, and undoing that on discard cost the same again. Only the
  rows a plan touches differ now; the rest recede by a rule that costs nothing
  per row, so the table is patched where it changes instead of thrown away.
  Measured on a library of 3799 files: staging three changes went from 4.3 s to
  0.26 s, leaving the staged state from 159 ms to 11 ms. A staged file the
  filter would hide still needs the full renderer, and still gets it. (#186)

- Staging an import no longer repaints the file table twice. Bringing a release
  into three files of a few thousand rendered every row, then rendered every row
  again to show the diff — the first one only to throw it away. The same applied
  to the field editor's Apply. The remaining cost, repainting untouched rows to
  stage a handful, is #186. (#186)

- The toast after opening a library counted in the plural whatever the number.
  (#187)

- **Auto-match no longer throws its answer away when the folder holds fewer
  files than the release has tracks.** Three files of a five-track EP matched
  correctly and were then silently demoted to filler, because a match to track 4
  or 5 had nowhere to go while all five tracks were still ticked — the import
  pairs the i-th enabled track with the i-th file. What reached the import was
  the original order against tracks 1–3, which would have tagged *Affairs Of The
  Heart* as *Delicious (Radio Edit)*.

  Matching now sets the ticks to agree with itself: exactly the matched tracks
  stay enabled, the files line up in their order, and the toast says how many of
  the release's tracks were left out. (#185)

- **A large library stops making every click expensive.** With a few thousand
  files open, selecting a row took a quarter of a second, select-all took a
  second, and the filter lagged a word behind the typing — the tag-field grid
  looked each selected file up by scanning the whole library, once per file and
  again per field, while the table repainted in full for anything at all.

  Files are indexed by path now, only the rows whose selection actually changed
  are repainted, and the filter renders once you pause instead of once per
  character. Measured on 3799 files: a row click **238 ms → 12 ms**, select-all
  **1063 ms → 93 ms**, and six keystrokes in the filter **~900 ms → 1 ms** of
  blocked typing. (#184)

- **One bad year no longer hides a whole file.** A handful of tracks in a large
  library listed as *"couldn't read tags — file left untouched"*, with nothing
  else about them visible. They were not damaged: each carried a malformed year,
  and the tag backend's default strictness rejects the entire file over that one
  frame. Which is the wrong way round — a file with a broken year is exactly
  what someone opens a tagger to fix, and every player reads it fine.

  Files are read leniently now: a frame that cannot be parsed is skipped and the
  rest of the tag comes through, so title, artist, key and tempo are there to
  see. The unreadable frame is absent rather than guessed at, and saving the
  file rewrites its text frames from what you see — which repairs it. (#183)

- **A path pasted from the file manager opens the folder now, and says so when
  it can't.** Finder wraps a path containing spaces in single quotes, so pasting
  one opened nothing and left an empty table that looked exactly like a folder
  with no music in it. A pasted path is cleaned up first — surrounding quotes of
  either kind, stray whitespace, and the backslash-escaped spaces a path dragged
  into a terminal carries — and the cleaned path is what the field then shows.
  Opening something that is not a folder is now an error with a message instead
  of an empty listing. (#179)

- Clicking the library indicator opened the path field *beside* it instead of in
  its place, so both shared the row and each was cut in half, and the bar shifted
  while the field was focused. The field now takes the indicator's place in a
  slot that keeps its size, so nothing moves. (#178, #181)

- A folder name too long for the bar ran under the recent-folders caret. The
  name now gets the whole width it needs: when the line won't fit, the parent
  path is dropped rather than shaved, so the bar reads either
  `…/Temp music/**folder**` or the folder name alone — never two ellipses and
  half a parent. The full path stays in the tooltip. (#182)

- The release badge quietens its track count. The catalogue number keeps the
  bundled monospace — it is an identifier, the slashed zero tells `0` from `8`,
  and the tabular digits line the chips up down a list of results — and the
  count beside it now sits a weight lighter, so the badge stops competing with
  the release title. Both halves stay in one face: a second one at the same size
  sits at a different x-height and pulls the pill apart. (#175, #176)

## [0.5.3] - 2026-08-16

### Added

- **A reorganize carries the rest of the folder with it.** Filing an album out
  of an unsorted folder used to take the tracks and their sidecars and strand
  everything else — the rip log, loose artwork under a name no sidecar rule
  matches, a `Scans/` subfolder — in a folder that could then never be tidied
  away, because it was never empty. Those files now travel to the same
  destination, subfolders included, and the emptied folder is pruned.

  Deliberately narrow, since this moves files you did not select: it happens
  only when **every** track under the folder is leaving in the same operation,
  and only when they all land in the **same** destination folder. A folder
  holding another album's music, or tracks fanning out to several destinations,
  is left exactly as it was. The preview says how many extra files it will take,
  undo puts all of them back, and Settings › Files › **Carry folder leftovers**
  turns it off. (#161)

## [0.5.2] - 2026-08-16

### Added

- **A Length column.** How long a track runs is the one thing about it the table
  could not show, and it is what separates an edit from an original or a
  truncated rip from a good one. Tick **Length** in Columns ▾: it reads `m:ss`,
  sorts on the actual seconds rather than on the text (so 9:59 comes before
  10:01), filters like any other column, and is read-only — a playing time is a
  property of the audio, not a tag. It costs nothing to open a library with it:
  the value comes from the same read that already collects the tags. (#172)

- **The Columns menu keeps its actions in view.** Add column…, Fit to content,
  Autofit and Reset to default sit under the column list, and with a dozen
  columns picked the list scrolled past them — nothing on screen said they were
  there. That block now stays pinned to the bottom of the popover while the list
  scrolls behind it, which also makes it obvious that the list *is* scrolling.
  (#174)

- Length is one of the columns a table starts with, next to artist, title, album
  and year. A set you have already arranged is remembered and still wins, so
  this only shows up on a fresh library or after Reset to default. (#173)

- **Drag a column header sideways to reorder the columns.** Until now that meant
  opening Columns ▾ and dragging the grips there; the direct gesture works in
  the table itself, with an accent rule showing which edge the column will land
  against. File stays first and is neither dragged nor dropped onto. The two
  header gestures it shares space with are untouched: a click still sorts, the
  right-edge grip still resizes, and a header that was dragged does not also
  sort. (#89)

### Fixed

- **Searching a metadata source no longer needs a folder open.** Looking a
  release up is something you do *before* choosing files, but the search, the
  release fetch and the cover fetch all went through the open library and
  refused with "no library open" until a folder had been picked. They now run on
  their own. Saving the images next to your tracks still needs a library, for
  the obvious reason. The request spacing is now one cadence per source for the
  whole session rather than being reset every time a folder is opened, so
  reopening one can no longer burst past a provider's rate limit. (#166)

## [0.5.1] - 2026-08-15

### Fixed

- **A file no longer states the same thing twice in two spellings.** Some
  taggers write the label, the release country and the media type into user-text
  fields of their own (`Label`, `COUNTRY`, `OriginalMediaType`) rather than the
  standard places TagRex uses (`TPUB`, `RELEASECOUNTRY`, `TMED`). Those were
  carried over as unrelated custom fields, so after an online import a file
  claimed two labels and two media types with different values, and the editor
  showed both. They are now recognized as the field they mean: the standard one
  wins where a file has both, a lone old-style value carries into it, and saving
  the file drops the leftover. Notation-dependent fields are deliberately left
  alone — a DJ tool's `KEY` may be Camelot where the standard frame is musical,
  so folding those would silently discard one of the two. Any other custom field
  round-trips exactly as before. (#171)

## [0.5.0] - 2026-08-15

### Added

- **Beatport as a third metadata source.** On digital electronic releases the
  general-purpose databases are thin: the mix name is often missing, sub-genres
  are coarse, and label-only digital catalogue was frequently never entered at
  all. TAGGER › ONLINE › Source now offers Beatport alongside Discogs and
  MusicBrainz, with the same search, release cards, cover browser and import.
  It carries what the others cannot: the mix name (kept in the track title, as
  the store spells it), the tempo, the musical key — normalized to the compact
  spelling, so the existing key-notation transform converts it to Camelot — the
  sub-genre, the label and catalogue number, and 1400px artwork.

  Beatport issues API credentials to partners only, so signing in uses the
  public client from its own documentation page, under **your** account:
  Settings › Beatport › Sign in opens Beatport's login page in a separate
  window, and TagRex only ever sees the authorization code that comes back —
  never a password. The access token is renewed automatically and stored with
  the app's configuration, never in the repository. Unofficial access can stop
  working without notice, which is why the source lives in its own crate: if it
  breaks, nothing else does. (#162)

- Tempo and key are now part of a release the app fetches, so an online import
  can write them. Both are per track and silent when the source doesn't state
  them, and both are individually switchable in Settings › Import fields.
  (#162)

### Fixed

- A release with one track read **"1 tracks"** on its card, and a search with
  one hit read "Found 1 entries". Both now agree with the number. Easy to meet
  on a store's catalogue, where single-track releases are the norm. (#167)

- The rest of the counted messages stopped hedging with `(s)`: previews, toasts
  and the count next to each panel's title now read "1 file" or "3 files",
  "1 track" or "12 tracks", across the cover well, the generator, the field
  editor, the renamer, the exporter, the deduplicator and the online import.
  (#169)

- The browser-development mock counts its cover images the same way, so a plan
  description read there matches what the real backend produces. (#170)

- **The tempo never reached a FLAC or an Ogg file, and the label never reached
  an M4A.** Both were written under a tag item that only ID3v2 (and, for the
  tempo, MP4) can hold, and a tag save silently discards an item the format has
  no field for — so the value was simply absent afterwards, with no error
  anywhere. The write path now picks the item each tag type actually knows:
  `TBPM`/`tmpo` for the tempo where they exist and plain `BPM` on Vorbis, the
  label under the key MP4 maps, and the year under APE's own. Files already
  written this way are unaffected; re-saving them fills the missing values in.
  A new test asks the tag backend directly, for every field and every tag type,
  whether the value can be stored at all, so this class of silent loss cannot
  return unnoticed. The tempo and the musical key remain unwritable on APE tags
  (Musepack, Monkey's Audio, WavPack), which map neither. (#165)

### Changed

- A Beatport search now cleans the query before sending it. The store takes one
  free-text box and matches it loosely, so the `-` between artist and album, and
  disc markers like `CD1` that a folder name carries, were matching as content
  and burying the release actually being looked for. Punctuation becomes spaces
  (apostrophes are dropped, so `90's` stays one word) and disc markers are
  removed. Only this source is affected — the other two take structured fields.
  (#168)

- Reworded the provider-boundary module comment so it makes its point about
  crate isolation without naming a third-party application. (#163)
- Swept the rest of the committed text for the same slip: four code comments,
  the architecture document (including its prior-art section, now written as
  classes of tool rather than a list of products) and three older changelog
  entries no longer name comparable applications. (#164)

## [0.4.0] - 2026-08-13

### Added

- **A file can now carry several images, each saying what it depicts.** The
  model held exactly one picture — the front cover — so a release's back cover,
  disc scan or leaflet could not be kept even though every tag format allows
  them, and opening such a file and saving it dropped all but one. EDITOR › Cover
  art keeps the well for "the cover" and gains a strip below it listing the rest:
  each image with its type (front, back, disc, leaflet, artist, label logo, icon,
  other), a grip to reorder, and a ✕ to remove, plus **Add image…**. Embedding a
  picked, dropped or fetched image now replaces the *front* one and leaves the
  others alone, rather than throwing the set away.

  The strip appears only when every selected file carries the same set — same
  images, same types, same order — because with anything less there is no single
  set to edit. Every edit stages the whole revised set as one change, which is
  also exactly what undo writes back. A journal written by an older build still
  rolls back: its single old/new pair reads as a one-image set. (#56)

- **Reorganize can file tracks into a folder outside the opened library, and can
  copy instead of moving.** The target path was always built under the open
  library, so the workflow this feature is most wanted for — an unsorted folder
  that gets tagged and then filed into the real library, which lives somewhere
  else — could not be expressed at all. RENAMER › Reorganize now has an **Into**
  destination (empty = the opened library, exactly as before), a **Move / Copy**
  choice, and a **tidy up empty folders** option that removes the folders a move
  leaves behind. A copy leaves every source file untouched and undoing it removes
  the copies; undoing a move puts the emptied folders back so the files have
  somewhere to return to.

  Writing outside the library widens a deliberate safety boundary, so it is
  widened deliberately: the destination is only ever a folder you picked in the
  chooser — never something read out of a mask — and a plan aimed anywhere else
  is still refused wholesale before anything is written. The roots a batch was
  applied under are recorded with it, so a reorganize into an external folder is
  still undoable in a later session, when the app no longer knows about that
  folder. History lists a batch by where its files came from rather than where
  they went, so an external reorganize does not vanish from it. (#153)

### Removed

- **TAGGER › FROM NAME no longer has a cleanup chain of its own.** It had one
  because the values a mask extracts exist nowhere yet, so nothing that read the
  file could tidy them. A chain now runs over the staged plan instead, which
  does that job one step later and does it for every flow that produces values,
  so keeping a second copy in one panel was two mechanisms for one idea. The
  panel is back to the pattern and its read-out; cleaning up is **Preview tags**,
  then **Clean up** on the preview bar. The read-out now shows what the mask
  actually reads, raw — there is no plan to clean while a pattern is being
  typed, and a pattern is tuned against what the name says. A chain saved under
  the old panel is dropped; the saved action groups it could load are untouched
  and still there in GENERATOR. (#159)

### Added

- **A rule chain can now run over the staged changes, not just over what is on
  disk.** Every flow that produces values needs the same second step — clean
  them up — and that step could only ever read the file, so producing and
  cleaning meant writing first and transforming afterwards: two previews, two
  Applies and two undo entries for what was one operation. Values that exist
  nowhere yet, like tags a mask has just read out of a file name, could not be
  cleaned at all. While a plan is staged, the chain reads the plan instead: each
  group sees the value the plan proposes for the field it is scoped to, and the
  revised plan replaces the staged one, so it stays one Apply and one undo entry.
  The floating diff bar gets a **Clean up** button that opens the same chain the
  toolbar wand does — one chain, one set of rules, two ways in — and the panel
  says which of the two it is about to act on. A cleanup that lands back on the
  file's current value stops being a change and leaves the plan; a file-scoped
  chain revises the rename the plan proposes rather than adding a second one,
  and the sidecar files follow the revised name. (#142)

### Fixed

- **The Groups menu no longer opens off the bottom of the window.** It was
  positioned by CSS, always below its button, which is fine for a toolbar and
  wrong for the two places the button now sits low on screen: FROM NAME's pinned
  footer, and the transform popover when that is anchored on the floating diff
  bar. In a 920px window with only two saved groups the menu already ran 29px
  past the edge; with a real shelf of saved groups plus the built-in ones, most
  of the list was unreachable. It is now placed against its button from JS and
  flips above it when there is no room below — the same treatment the placeholder
  reference and the transform popover already had, now one shared helper. It
  follows the button when the panel behind it scrolls, and the toolbar menus,
  which have room below them, are left as they were. (#160)
- **A track's several artists or genres are no longer silently reduced to the
  last one.** Both ID3v2.4 (several values in one frame) and Vorbis comments (a
  repeated key) can say a track has two artists, and reading such a file kept
  only whichever came last — so the file looked wrong, and any edit at all, even
  to an unrelated field, wrote that single value back over all of them. The
  extras were gone with no sign they had ever been there. Multiple values now
  reach the app as one string joined by a separator (`; ` by default,
  configurable under Settings › Tag defaults) and are written back out as
  separate values: separate comments in a Vorbis file, one properly formed
  multi-value frame in an ID3v2 one, not the out-of-spec duplicate frames most
  players ignore. The joined form is the canonical one, so the table, masks and
  exports show and use it with no special handling. Only Artist, Album Artist,
  Genre and Composer are treated this way — splitting any other field would turn
  a title reading `Hello; Goodbye` into two titles. Verified on a real file
  tagged by other software: two artists in one frame read back joined, an
  ordinary title edit left the frame byte-identical, and the ReplayGain data
  survived. (#46)

### Changed

- **TAGGER › FROM NAME now cleans up its extracted values with the same rule
  chain and action groups as GENERATOR.** The panel had a replacement table of
  its own: two mechanisms for one idea, in neighbouring panels, and the flat
  table could only ever do a literal find-and-replace over every value at once.
  It now shows the same rule cards — find and replace with regex and whole-word,
  change case, remove diacritics, transliterate either way, key notation — and
  the same Groups popover, backed by the same saved groups in settings.json. A
  group's scope is read here as *which extracted value* it acts on, so ticking
  one scoped to Artist and another to Title gives per-field cleanup, which the
  table could not express at all. The live read-out follows the chain as it is
  edited, and the chain persists across sessions like the pattern does; a
  replacement table typed under the old panel carries over as ordinary replace
  rules. Groups scoped to the file name or its extension are hidden, since
  neither exists among the values a mask extracts. (#144)

### Fixed

- **The Folder name and File name query presets no longer search for
  underscores.** Downloaded music is routinely filed as
  `various_-_la_bush_-_music_from_the_temple_of_house_(as_5606)_(1996)`, and that
  string was handed to the provider verbatim — where it matches nothing, which
  reads as "this release isn't in the database" rather than "that isn't a query".
  The presets exist to save typing and were making you retype the whole thing.
  Both now replace underscores with spaces and collapse the result. Verified
  against the live search: the raw folder name returns no hits, the normalised
  one returns the right release. Dots are deliberately left alone, since they
  carry meaning in `Ltd.` and `Vol. 2`, and the tag-derived presets are untouched
  — an underscore in a tag was put there on purpose. (#158)

### Changed

- **A single-disc release now imports as disc 1 of 1.** Writing no disc at all
  when a release stated none was too blunt: a release whose format quantity reads
  1 is not silent about the disc, it says there is exactly one, which puts every
  track on disc 1. The effect was an ordinary single-CD album importing with an
  empty DISC column beside files that do carry a disc, sorting and grouping as
  missing rather than as one. The count is what licenses it — on a release
  stating two or more discs, a track whose position names no disc is genuinely
  unplaced and is still left alone, and a release stating no count at all still
  writes nothing. The per-field switch in Settings › Online import turns it off
  for anyone who would rather not have disc numbers at all. (#157)
- The folder-name fallback for a disc now only accepts 1–99. Box sets do not
  reach three digits, so a larger number means the keyword happened to be
  followed by digits about something else — a directory named
  `…-single-disc-99831-…` was reading as disc 99831. (#157)

### Fixed

- **Column layout persists again, and header-grip resizing works again.** Three
  constants — the two storage keys and the minimum column width — were left
  behind in `app.js` when their users moved into their own modules, so in
  `columns.js` and `tablegestures.js` they were undefined identifiers. Dragging a
  grip threw on the first mouse move and the column never changed width; saving
  and loading the column set and widths threw inside `try` blocks written to
  tolerate an unavailable `localStorage`, which swallowed the error and returned
  silently. The effect was that which columns you show, their order and their
  widths were all discarded on restart, and it looked like a feature that had
  never been finished rather than one that had broken. The constants now live
  with the code that uses them, and those `try` blocks have been narrowed to the
  storage call they were meant to guard, so the next mistake of this shape is
  loud instead of invisible. (#155)

### Added

- **The transform chain is reachable from any mode.** A wand button in the
  toolbar opens the GENERATOR rule chain as a popover, so a cleanup can be
  composed and run without leaving the mode that created the mess — which is
  almost always where it was made. Fixing the case of tags just read out of file
  names in TAGGER previously meant switching to GENERATOR, re-selecting the
  intent, and coming back. It is the same chain rather than a second copy: the
  panel block is moved into the popover and back out again, so one set of rules
  and one set of controls exist and the two entry points cannot drift apart.
  Saved action groups come with it. Preview closes the popover and stages the
  diff on the table behind it; an error leaves it open so the chain can be fixed
  where it stands. The button is hidden in GENERATOR itself, where the chain is
  already on screen. (#149)

- **A multi-disc release now imports with its disc numbers.** The disc was only
  ever derived from a vinyl side letter, so a 2×CD imported as if it had no
  discs at all — every track landed with no disc, and reorganizing by
  `%disc%/%track%` had nothing to work with. Both providers state it and both
  were being dropped: Discogs encodes the disc in the track position (`1-05`,
  `CD1-3`) on a flat tracklist, and MusicBrainz keeps it as the medium, which was
  thrown away when the media were flattened into one track list. Both are now
  read, along with the disc count — Discogs' format quantity (`2×CD`), the number
  of MusicBrainz media — written as the "of N" half of the pairing. Failing both,
  the file's own folder is used as a last resort (`CD2`, `Disc 3`), but only when
  the file has no disc yet, and only on a real keyword: a folder that merely ends
  in a number is not a disc, since compilation series are routinely filed as
  `… (1996) 2` where the 2 is the volume. A vinyl side is still a side, not a
  disc, and mapping one to the other stays the explicit per-import toggle it was.
  Nothing is written when a release states nothing — a single-CD album gains no
  disc tag, and no lone `disctotal` of 1 either, since a disc total with no disc
  number to complete says nothing. (#146)

- **A placeholder reference inside the app.** Every pattern box — Rename by
  mask, Reorganize into folders, Tags from the file name, the Report export —
  gets a **?** button that opens the full list: every placeholder the parser
  accepts, grouped into Tags / File / Technical / Special, one line of
  description each, and the grammar (`%field:2%`, `[…]`, folder separators,
  quoted literals) below it. Clicking one inserts it at the caret rather than at
  the end, since a pattern is usually edited in the middle, and a filter box
  narrows the list. Hovering a table column header now names the placeholder
  that addresses that column, so the table answers the question where it is
  asked. Previously the only list inside the app was one hint under the RENAMER
  box that named nine of the nineteen tag fields — and `%catalognumber%` was not
  among them, which left guessing at `%catno%` or `%cataloguenumber%` as the only
  way to find it, and left anyone offline with no way at all. That hint has been
  rewritten to point at the button. The list is built from the same tables the
  parser reads, so a name it shows is by construction a name that parses, and
  the tests fail if the two ever disagree. (#148)

- **Masks can now address the file, not just its tags.** Sixteen new
  placeholders: the path ones — `%filename%`, `%fileext%`, `%filenameext%`,
  `%filepath%`, `%foldername%`, `%foldername2%`, `%foldername3%` — and the
  technical ones, which carry a leading underscore to mark them as properties of
  the audio rather than of the file's place on disk: `%_length%`,
  `%_length_sec%`, `%_bitrate%`, `%_samplerate%`, `%_channels%`, `%_codec%`,
  `%_filesize%`, `%_filesize_bytes%`, `%_filedate%`. They work anywhere a mask
  does — rename, reorganize, report — which is what makes an export like
  `%artist% - %title% [%_bitrate%kbps %_length%]` expressible at all, and what
  lets a rename keep the folder a file came from. All of them are render-only,
  for the same reason `%side%` is: there is no tag to read a bitrate back into,
  and pulling `%filename%` out of a filename says nothing, so a mask carrying one
  is refused by FROM NAME rather than quietly matching. A value that isn't
  available renders as empty instead of failing the file — an unreadable
  property, a folder level above the root — so a pattern stays usable across a
  mixed selection. Duration is `m:ss`, size is human-readable, and the date is
  UTC so the same file renders the same name on any machine. The two that cost
  a read (a probe for the audio properties, a `metadata` call for size and date)
  are only paid for by patterns that actually ask for them, so the ordinary
  tags-only mask is exactly as fast as before. (#147)

- **Tags can now be read out of the file name.** The mask grammar has always
  been bidirectional — the same pattern that renames a file from its tags can
  read tags back out of a name — but only the renaming half was ever reachable,
  so a library that arrived as `01 - Artist - Title.flac` with empty tags could
  be renamed and not tagged, which is backwards: the names already held the
  metadata. TAGGER gets a third sub-tab, **FROM NAME**, beside ONLINE and
  EDITOR, since tags are the outcome. The same placeholders as RENAMER, plus
  `%skip%` for a run of text that maps to no tag, and separators to reach into
  the folders — `%albumartist%/%album%/%track% - %title%` matches one folder
  level per separator, because that is where the artist and album usually are.
  Under the pattern box a live read-out shows the string being matched and what
  the pattern pulls out of it, so a mask can be tuned without staging anything.
  Pressing Preview produces an ordinary plan: the same in-table diff, the same
  Apply, the same undo. A name that doesn't fit the pattern is left alone
  rather than failing the batch, and a value the field can't hold — a vinyl
  `A1` as a track number — is flagged in the diff instead of being written.

- **FROM NAME gained a replacement table.** Names carry their separators as
  junk, so `the_x_factor` was landing in the Artist tag verbatim, and fixing it
  meant a second pass — extract, apply, then a GENERATOR chain over the same
  files — two plans and two undo entries for what was one operation. A small
  table under the pattern box now runs literal find-and-replace over every
  value the mask extracts, before it becomes a change. It starts with the row
  everybody needs, underscore to space, which is an ordinary row and can be
  deleted. The live read-out shows the replaced values, since that is what
  would be written, and the pattern and the table both come back next session.
  Regex and whole-word matching stay in GENERATOR: this is the quick pass, not
  a second transformation engine. (#141)

- **A stated width now splits values that run together in a name.** Real
  numbering packs the disc and the track into one run — `101_artist_-_title` is
  disc 1, track 01 — and the pattern for it, `%disc:1%%track:2%`, was rejected
  as ambiguous. That rejection is right for `%disc%%track%`, where nothing says
  where one value ends (#65 refused to guess), but a width written out says
  exactly that, and the extract direction was throwing the information away.
  Widths stated in the pattern are now fixed lengths when reading a name;
  adjacent placeholders are refused only when neither of the pair states one.
  A width the parser supplies itself is unchanged — `%track%` pads to two
  digits when renaming and still matches a plain `5` when reading — and on the
  integer fields a fixed-width run has to be digits, so a pattern that doesn't
  meet a number there misses cleanly instead of capturing two letters as a
  track number. (#140)

- **A file-extension transform scope.** Rules could reach every tag field, one
  tag field, or the file name's stem — the extension was the one part of a
  file's name nothing could touch, so retyping a shouty `.FLAC` meant a rename
  mask, which rewrites the stem along with it. `File extension` is the mirror
  image of `File name`: the chain sees the extension without its dot and the
  stem is carried through untouched. The result is an ordinary rename plan, so
  preview, apply, undo and sidecar carry-along work as they already do. An
  extension that would come out holding a separator or a dot is refused — that
  moves the file or changes how many extensions the name has — and a file
  without an extension is skipped rather than given one.

- **Eight transformation presets now ship with the app**, instead of the shelf
  starting empty and every chain having to be built by hand: Standard values,
  Discogs cleanup, Normalize english, General Latin, No dash, Lower case, File
  extension and FTP format. They are ordinary action groups — same steps, same
  scopes, run through the same preview and undo — except that they live in the
  app rather than in your settings, so they can't be deleted and can't drift.
  Load one to copy its steps into the chain, edit them and save under your own
  name; the preset stays as shipped. Every one is a chain you could have built
  yourself: none of them needed a rule kind that didn't already exist.

- **Reverse transliteration — Latin back to Russian Cyrillic**, for tags that
  arrived already romanized by a ripper or an earlier tagger. Reversing a
  romanization is guesswork, so the step is built to be wrong as rarely as
  possible: longest run first, so `shch` is щ before `sh` reaches ш; and a word
  is all-or-nothing, left in Latin if any letter has no Cyrillic reading. That
  is what keeps "Jazz" and "The" from turning into "Jазз" and "Тхе". What the
  forward direction discarded stays discarded, and the rule card says so
  outright: `ъ` and `ь` romanize to nothing, `й` and `ы` both come back as `й`.

### Changed

- **The frontend is a set of modules rather than one 6,700-line file.** Every
  feature area — the player, the settings sheet, ONLINE, the field editor,
  GENERATOR, RENAMER, the exporters, the deduplicator, cover art, FROM NAME,
  the columns, grouping, the drag gestures and the browser-only mock — is its
  own ES module now, and the state only one of them uses is private to it. No
  behaviour changed; this is a move, not a rewrite. What it buys is that the
  next change to one panel can no longer quietly reach into another. (#143)

- **Action groups run as a checklist rather than one per click.** A cleanup is
  usually two or three groups in a row, and running them one at a time meant
  previewing and applying each in turn. Each row is now a tick, the group's
  name over **the scope it acts on**, and Load; the footer runs everything
  ticked, in list order, as a single plan. This also fixes a way the old
  behaviour was simply wrong: run separately, each group was computed against
  the file on disk, so lower-casing the file name and then rewriting its
  extension had the second change silently discard the first. Composed, they
  end in one rename.

### Fixed

- **A transform that changes nothing now says so**, instead of reporting
  `TypeError: Cannot read properties of null (reading 'changes')`. The message
  was already written; it just couldn't be reached. An empty plan makes the
  preview leave the diff state, which clears the staged plan, and the line that
  builds the toast was still reading it. Both the rule chain and the group
  checklist reported it. (#145)

- **The Groups popover was clipped by the panel it opens in.** It is
  right-aligned like every other menu of its kind, which suits a button near
  the right edge but not this one, at the left of a narrow column — the menu
  ran past that column's edge and was cut off there. Previously that cost the
  footer's text box; with a list of named presets it would have cost the names.

## [0.3.0] - 2026-08-08

### Added

- **The player grew the controls it was missing: Previous, Next, Repeat and
  volume.** Prev/Next step through the table's visible order; Repeat has three
  states — off, repeat all (wrapping at the end of the list) and repeat this
  track, shown by the icon tinting and a small "1". Volume gets a slider and a
  mute toggle that remembers the level to come back to, persisted between runs.

### Changed

- **Grouping moved to an icon button with a structured menu.** The labelled
  select spent about 130px of the toolbar on a list that runs to every modeled
  field, with no ordering to it. The menu now promotes the keys actually reached
  for — None, Folder, Release id, Artist, Album, Album Artist — above a
  separator, with the remaining fields below, and ticks the active one. The
  current key stays answerable without opening anything: the table renders group
  headers, and the button tints while grouping is on.

- **Clear tags is reachable from the table toolbar**, instead of only from
  EDITOR. Same operation, unchanged in scope: modeled text fields only, cover
  art and DJ cue points kept, and staged as a preview so Apply/Discard still
  gates it. It sits at the end of the row behind a divider, because everything
  to its left configures the view while this one acts on the selection.

- **The player row now appears with a track and leaves with it.** It used to be
  revealed on the first playback and then stay for the rest of the session,
  spending a footer row on a bar reading "No track loaded" — deliberate, so a
  Play control was always on screen. The row now comes and goes with playback
  and the status bar carries the Play control while it's down, so nothing is
  lost and the row isn't standing rent-free.

- **The player names the track from its tags** — "Wish Mountain — Radio" rather
  than "102_wish_mountain_-_radio.mp3" — falling back to the file name when the
  tags are empty.

### Fixed

- **Changing Repeat mid-track had no effect until the track after next.** The
  backend asked for the next track the moment playback began, so the following
  source was already appended to the audio sink seconds in — and an appended
  source can't be taken back. Switching to "repeat this track" was therefore
  ignored: the current track ended, the next one started, and only *it* went on
  to repeat. The queue is now primed a few seconds before the end of the track
  instead of at its start, which leaves the decision open for practically the
  whole track while still being ample lead time to stay gapless.

- **Auto-advance stopped at a group boundary.** The "next visible row" walk
  counted every table row, so with grouping on it landed on a group header —
  which carries no path — and playback simply stopped rather than continuing
  into the next folder. It now steps over headers and collapsed rows.

- **The seek slider changed size with every track.** The track name shared its
  row and was sized by its content, so the slider absorbed whatever was left:
  268px wide for a short title against 112px for a long one. Beyond the layout
  jumping on each advance, that changed how much time a given drag distance
  covered. The name now occupies a fixed column and ellipsises, so the slider
  keeps its geometry.

## [0.2.0] - 2026-08-07

### Added

- **One bundled type superfamily for portable, legible text.** UI text now
  renders in **IBM Plex Sans** (weights 400 / 600 / 700) and the optional
  condensed table font in **IBM Plex Sans Condensed**, both bundled as
  Cyrillic-covering woff2 subsets (`app/ui/assets/tagrex-sans*.woff2`,
  `tagrex-ui-condensed.woff2`). Type now looks identical on macOS, Linux and
  Windows instead of drifting across each OS's system sans, and stays fully
  offline. The disambiguating **JetBrains Mono** is kept for the aligned data
  columns, so the app reads as one system with a deliberate mono exception.
  `--font-ui` leads with the bundled face and gains a real Linux fallback
  (Cantarell / Noto Sans) before the generic. This lands the bundled condensed
  face the table toggle had been falling back to a system face for (#100). All
  faces are OFL 1.1, subset to Latin + Latin-ext + Cyrillic + digits + the
  punctuation common in filenames.

- **Settings › LAB — a section for typography still being trialled.** Marked
  _experimental_ and stated as such, so it's clear these may change or be
  dropped rather than sitting among settled preferences. It collects the value
  font (moved out of Display) and the table font size, and adds two new knobs:
  a **tracklist font size** (10–16px, default 12) and a **release badge font**
  (mono or sans) for the catalogue-number and track-count badges. All apply
  live.

### Changed

- **Release-card toolbar rebuilt around what each control actually is.** The row
  held six lookalike buttons mixing three different kinds of thing. **Enable all
  / Disable all** weren't actions at all but a scope, so they become a single
  master checkbox — tri-state, showing whether all, some or none of the tracks
  are on — moved into a new sticky tracklist header where it shares a column
  with the per-track boxes and lines up with them by construction, taking the
  `N / M selected` tally with it. **Save cover / Save all (N)** were one call
  with a boolean, so they become one narrow artwork button carrying the image
  count, with the variants on its caret. **Expand all / Collapse all** are a
  state toggle, so they become one icon button offering whichever move the
  current state allows. The mode-panel collapse button joins Undo and Settings
  in the top bar instead of sitting alone at the end of the mode-tab row.

- **Cover resolution now qualifies the action it belongs to.** The row printed
  `600×594 · 12 images`, which read as a property of the whole set; the figure
  is in fact the primary image's, the one `Save as folder.jpg` writes and
  `Embed cover` embeds. It moves onto that menu item and the button's tooltip,
  and the loose readout leaves the row.

- **Search and results columns line up.** The results pane reserves a scrollbar
  gutter, so its contents sat 11px further left than the search controls above
  (25px vs 14px from the panel edge). The scrollbar width is measured once into
  `--sb-w` — zero on overlay-scrollbar platforms — and the header reserves the
  same gutter, so the two edges agree everywhere.

- **One control tier and one type rule inside the release card.** The card's
  toolbar ignored the design system's control heights entirely — `.btn-sm` set
  only padding, so height fell out of the content and a single row held 18.5px,
  21px and 24px controls at three different type sizes (10 / 11 / 12px), with
  mono and sans alternating on no stated rule. Every control in that row is now
  pinned to `--ctl-h-sm` (24px) at 11px — the tier the Label · cat# select in the
  same card already used — and the card follows one rule: **sans for chrome and
  labels, mono for data and identifiers**, at two sizes (11px dense layer, 12px
  card title). The catalogue/track-count badges go 10px → 11px and the tracklist
  rows 11px → 12px. **Preview edits** is brought onto the standard 28px / 12px
  tier: it had no height of its own and inherited the status bar's 11px, so in a
  narrow bar its label wrapped to two lines and the button stood 43px tall.

- **Settings › Display now picks the value font app-wide: Mono / Sans /
  Condensed.** The old **Condensed table font** checkbox only ever restyled the
  file table, and offered no way to drop the monospace look elsewhere. It is
  replaced by a three-way segmented control (styled like Theme, and live — the
  swap applies on click, not on Save) whose choice redefines the bundled-mono
  token itself, so every value surface follows it: the file table, the release
  tracklist, deduplicator paths, rename/export pattern fields and the editor
  inputs. **Mono** (default) keeps columns aligned and `0`/`O` distinct;
  **Sans** renders values in the same IBM Plex Sans as the rest of the
  interface; **Condensed** fits more in before a value truncates. An existing
  condensed preference migrates to the new setting automatically — note it now
  applies everywhere, not just to the table.

- **Palette nudged to clear WCAG AA in both themes.** The dark-mode accent fill
  is darkened (`#0d8f6a` → `#0b7d5c`) so white primary-button labels pass AA
  (they were at 4.07), light muted text is darkened (`#6b7280` → `#636b76`) for
  margin over the panel colour, and the dark diff/success green is softened
  (`#4ade80` → `#3fca74`) so its weight matches the light theme. Accent-as-text
  (`--accent-ink`) is unchanged, and the accent stays anchored to the brand
  green — a contrast tune-up, not a rebrand.

## [0.1.0] - 2026-08-06

First tagged release. Everything below is the work that brought the editor to a
usable cross-platform state; subsequent entries will accrue under _Unreleased_.

### Added

- **Sidecar files follow a rename or move** (#58). When a rename/move relocates a
  track, files sharing its name — `.lrc` lyrics, `.cue` sheets, `.txt` notes,
  per-track cover images — now travel with it and are restored alongside it on
  undo (they're journaled as part of the plan). Sidecars are shown on the staged
  row as a "+N sidecars" badge that names each pair on hover, and an existing
  file at the destination is never overwritten — the whole plan is rejected
  instead. A new **Settings › Files** section toggles the behaviour (on by
  default) and edits the extension set (default `lrc cue txt jpg jpeg png`);
  matching is case-insensitive. Extends the folder-restructure work in #37.

### Changed

- **Group the table by any tag field** (#43). The Group control offered only
  Folder / Artist / Album / Release id; it now lists every modeled field
  (Composer, Year, BPM, …) alongside those, so a column you add is groupable the
  same way the built-ins are — finishing the configurable-columns work. Empty
  values bucket under a friendly `(no <field>)` header, and an unknown persisted
  choice falls back to Folder.

- **EDITOR tag fields split into Core / Standard / Advanced** (#136). The form
  crammed every field into one flat, ellipsis-truncated list, so long names were
  silently cut (`OriginalMediaT…`, `ReplayGainAlbu…`) and raw technical keys
  (`DISCOGS_RELEASE_ID`, `REPLAYGAIN_*`, `1T_TAGGEDDATE`…) sat among the friendly
  fields. Fields now live in three groups that share one label-left / field-right
  layout: **Core** (the everyday fields), **Standard** (curated known fields,
  with a dictionary that promotes recognised technical frames under friendly
  names — Audio file URL, ReplayGain (album), Discogs Release ID…), and a
  collapsed **Advanced** holding whatever raw keys remain. Labels wrap in a
  slightly wider column instead of truncating (full name on hover), so nothing is
  clipped. Track and Disc render as one combined row of number/total pairs
  (`Track [n] / [total]   Disc [n] / [total]`), which added a first-class
  **disc-total** tag field to the backend so the "of N" half has somewhere to
  write. The whole form is denser — compact numeric boxes and shorter input rows
  — with the text size left unchanged.

- **Cover-art buttons share one size** (#134). Set one… / Remove all / Export and
  the full-width Use folder image were 10px vs the base 13px; they now sit at a
  consistent 12px with matching padding, so the section reads as one button
  group.

- **One focus outline across form controls** (#135). Focus now shows a single
  accent ring that hugs the control's contour — no gap, following its rounded
  corners — on every control alike. Before, a focused field drew an accent border
  _and_ an offset ring (a confusing double outline with a gap), and a clicked
  `<select>` fell back to WKWebView's own tight ring — the provider `<select>`
  also re-declared its border at ID specificity, which beat the shared focus rule
  and left it grey. The `--ring` dropped its `--bg` offset layer, the
  accent-border-on-focus is gone (the ring alone is the indicator), the native
  outline is suppressed, and the provider select inherits the base treatment.

- **A minimal right-click menu replaces the webview's native one** (#132).
  Right-clicking the app no longer pops _Reload / Inspect Element_ and the wall
  of macOS text services; over a text input or an editing tag cell a compact
  **Cut / Copy / Paste / Select All** menu shows instead, and elsewhere
  right-click does nothing. On macOS this also stops a Ctrl-click (an OS-level
  right-click) from opening a menu — the additive selection modifier there is ⌘.

- **Shift on the file table now range-selects, without the blue text drag**
  (#131). Shift-clicking a row or a group header extends the selection from the
  anchor through the click, selecting everything in between (⌘/Ctrl stays
  add/toggle). The browser's native text selection no longer paints over the
  cells while modifier-clicking.

- **Double-clicking a group header selects just that folder** (#130), replacing
  the selection instead of adding to it, and **expands the group** so the choice
  is visible. Hold **⌘/Ctrl/Shift** (with a click or double-click) to add a
  folder to the current selection instead. A plain single click on a header still
  does nothing, so a stray click can't wipe the selection.

- **Folder group headers show the path from the session root** (#129), e.g.
  `Album/CD1` instead of a bare `CD1`, with files directly in the root under the
  root's own name. Nested folders now read as a tree and same-named subfolders
  are told apart.

- **Only the first file is selected when a session opens** (#128), instead of the
  whole library. This stops an operation from silently spanning every file, and
  matches how comparable taggers behave. The user then picks what to act on — a
  row, a Shift-range, a whole folder via its group header, or the header
  select-all box. Applies to Open, Browse, and drag-and-drop alike.

### Added

- **Drop an image onto the window to embed it as the cover** (#133), now working
  in the packaged app. The cover well's embed-on-drop previously relied on HTML5
  file DnD, which Tauri's `dragDropEnabled` suppresses — so it only worked in the
  browser dev mock. The single native `tauri://drag-drop` listener now routes a
  lone image (any drop location — an image can't be "opened") to embed it into
  the selection, via a new `read_cover_image` backend command feeding the usual
  preview → apply → undo; everything else opens as a library/file-set.

- **Drag-and-drop folders and files onto the window** (#127). Dropping content
  onto the app now opens it directly, alongside Browse / Open. **A single folder**
  opens as a library (recursive scan, as before). **Files, several folders, or a
  mix** open a **file-set** session that lists and operates on *only* those files
  — each dropped folder is expanded into its audio files, loose files are kept as
  they are, and writes stay confined to the files' common ancestor. In a file-set
  the table groups by drop origin: **each dropped folder is its own group**, and
  loose files collect under a **`Files`** group. A dashed overlay cues the drop
  while dragging.

- **Named action groups: save and replay a transform chain** (#57). The GENERATOR
  transform chain can now be saved as a **named group** and re-run over any
  selection in one go — the whole group previews and applies (and undoes) as one
  batch, like a single transform. Groups persist in `settings.json` (a new
  **Groups** popover: Run / Load / Delete + save-current-as). Each step also gets
  an **enable/disable** toggle, so a step can be kept in the chain (and the saved
  group) but skipped.

- **HTML and XML exporters** (#42). The EXPORTER gains two formats beside
  Playlist / CSV / Report: **HTML** writes a self-contained, styled table (no
  external assets — opens in any browser) and **XML** writes a `<library>` of
  `<track>` elements (one child per non-empty tag, plus file + path). Both share
  the CSV column set and are pure, tested string builders confined to the library
  root like the other exporters.

- **Release card: cover resolution, and save cover / save all images** (#102).
  An expanded release now shows its primary cover's resolution and image count
  (e.g. `600×594 · 3 images`, from the provider's image data — Discogs reports
  dimensions, MusicBrainz doesn't). **Save cover** writes the primary image next
  to the selected tracks as `folder.jpg` (the name the app auto-reads as external
  art); **Save all** writes every image (`folder.jpg`, then `cover.jpg`,
  `cover-1.jpg`…). Existing files prompt an overwrite confirmation before
  anything is written. Backed by a new `save_release_images` command confined to
  the open library root.

- **Transliterate non-Latin scripts to Latin** (#72, GENERATOR transform). A new
  **Transliterate to Latin** transform step maps Cyrillic and Greek onto Latin
  (`Пётр → Pyotr`, `Ελλάδα → Ellada`) and composes into the same chain as
  replace / case / diacritics. It's distinct from **Remove diacritics** (which
  only strips accents off Latin letters): this maps a different alphabet.
  To-Latin only; per-script tables (BGN/PCGN-style) make adding another script a
  data-only change.

- **`%skip%` discard placeholder in filename→tags masks** (#70). The mask engine
  gains `%skip%`, which on the *extract* direction matches and throws away a run
  of text (filename junk that maps to no tag) and may repeat. It's the mirror of
  the render-only `%side%`: a mask carrying `%skip%` is extract-only, so the
  render direction refuses it with a clear message. (Core engine + tests; the
  filename→tags UI that will use it is a separate piece.)

- **Click a release's catalogue number to open its provider page** (#92). The
  accent-tinted catalogue segment of the release badge is now a link: clicking it
  opens the Discogs / MusicBrainz **release** page in the system browser. A new
  `open_release_page` command builds the URL from a hard-coded host plus a
  charset-validated id, so only provider release pages can ever be opened.

### Fixed

- **The Undo button no longer reverts to a text label after opening a library**
  (#119, regression from #115). The icon-only Undo control was overwritten with
  "Undo (N)" text as soon as the history refreshed; the batch count now lives in
  its tooltip / `aria-label` and the icon stays put.

- **Mode-panel selection counts stay in sync.** The GENERATOR and EXPORTER panel
  headings ("Number tracks — N selected", "Transform — N file(s)", "Export — N
  track(s)") only recomputed their count when you entered the mode, so changing
  the selection with the panel already open left a stale number. They now update
  live as the selection changes.

- The bottom **Play button no longer resumes the wrong track**. After pausing a
  track and then selecting a different one, pressing Play started the *paused*
  track instead of the newly-selected one. Play now pauses only while actually
  playing; when paused or idle it plays the current row (resuming when it's the
  same track, switching when it isn't).

- **Applying a change could silently drop the Year and Publisher**, even though
  the Preview showed them. Two ID3v2 round-trip bugs: (1) the year is a `TDRC`
  timestamp frame, which the writer didn't recognise as one of its own, so a
  file left with a stale or duplicate `TDRC` (e.g. two of them, from another
  tagger) leaked the old year back over the value just written; (2) lofty reads
  the publisher frame (`TPUB`) back as `Label`, so a written Publisher never
  round-tripped and vanished from the column. Both now persist correctly.

- **Import: artist names now use the Discogs release credit.** The importer took
  the canonical artist `name`; it now prefers the release-specific name variation
  (`anv`) when present — how the artist is actually credited on that release
  (e.g. `Wishmountain` → `Wish Mountain`) — and honours the `join` between
  credits (e.g. `Zolex Presents Carat Trax 3`). This stops needless changes to
  artists that already match the release credit.

- The **Catalogue #** change now appears in the Preview diff after importing a
  release. The diff only rendered a column for a fixed set of known extra fields
  and silently dropped any other change — catalogue number was written but never
  shown. It's now in that set (labelled "Catalogue #").

- Grid tiles no longer show a strip under the cover art. The cover image was
  rendered inline, leaving a baseline gap that revealed the striped placeholder
  below it, and a divider border added to the effect — the image is now a block
  and the divider is gone, so the art meets the info cleanly.

- The media badge no longer disappears when a release card is expanded — swapping
  in the full-resolution cover on expand used to overwrite the whole cover well
  and wipe the badge with it.

- The **GENERATOR rule chain** now reorders with the same pointer-based drag as
  the other lists (#88 follow-up), instead of HTML5 drag-and-drop that is
  unreliable in the app's WKWebView. Dragging a rule's grip moves it; the ↑/↓
  buttons stay as the fallback. This was the last list still on the old DnD.

- Native-app UI issues found in testing (#88). The **Columns** popover and
  **Read priority** list can be drag-reordered again — both used HTML5
  drag-and-drop, which is unreliable in the app's WKWebView, and are now on the
  same pointer-based reorder the file table uses. An **active segmented-control
  button** (EXPORTER format, Settings › ID3 version) no longer turns unreadable
  grey-with-white on hover — it keeps its accent and brightens. **List-view
  release cards** wrap the album title to two lines instead of truncating it
  with an ellipsis. The settings segmented controls no longer leave dead space
  after the last button. The side panel starts at 480px (min drag width 240px).

### Changed

- **The EDITOR's ADD FIELD control is compact and expand-on-demand** (#114). The
  always-present two-line block (lead label + name + Add + value) becomes a
  single quiet **+ Add field** affordance that expands into one inline row
  (name · value · ✓ · ✕) only when used, then collapses again — so it no longer
  competes with the field grid for space. Enter commits (staying open for
  several fields in a row), Escape or ✕ collapses, and the name input suggests
  the custom fields already present on the selected files.

- **The `Files (N)` label is gone; its count lives in the status bar** (#126,
  supersedes #121). The toolbar label duplicated the total already shown at the
  bottom, so it's removed and the status bar is now the single home for counts:
  `101 files · 15 selected`, or `12/101 files · 15 selected` under a filter. The
  toolbar controls (Group / Filter / Presets / Columns) left-align in its place.

- **Release badge padding evened out to 3px** (#125). A small follow-up to #124 —
  the segment padding was `2px 6px` (slightly letterboxed) and is now `3px` on
  all sides.

- **Release card: catalogue number and track count are one segmented badge**
  (#124). The two chips previously read as unrelated pills (one accent-filled,
  one hollow); they're now a single unit behind one unified border — an
  accent-filled catalogue segment and a neutral count segment split by a divider
  in the same border colour. Either stands alone (a release may have no
  catalogue number; the count fills in once fetched). Applies to the list card
  and the grid tile. Supersedes the #122 chip tweak.

- **Group headers label and separate their row count** (#123). A grouped file
  table showed a bare number after the folder/album/artist name (`music   3`);
  it now reads `music · 3 files` / `Artist · 1 file` — the app's standard mid-dot
  separator plus a pluralised label, so it's clear what the number counts.

- **Release card: the track-count chip now matches the catalogue chip** (#122).
  The `15 tracks` count chip read taller and chunkier than the `AS 5606`
  catalogue chip beside it (a 999px pill with wider padding vs a 4px rounded
  rect). They're now a true matched pair — identical height, font, padding and
  shape — differing only in colour (catalogue accent-tinted, count neutral).

- **The lone `Files` tab is now a plain label** (#121). With the Preview (#117)
  and Duplicates (#118) tabs gone, `Files` had nothing left to switch to, yet it
  still rendered as a clickable accent pill. It becomes a static, non-interactive
  label that still carries the track count (`Files (N)`); the dead `.view-tab`
  styling and its click handler are removed. The toolbar row (Group / Filter /
  Presets / Columns) is unchanged.

- **The Preview view dissolves into an in-table diff-state** (#117, third slice
  of the workspace redesign). A staged change plan no longer opens a separate
  mirror table behind a `Preview` tab — the file table itself shows the diff in
  place: changed cells light up (new value, with the struck-through old value on
  **Show old values**), a rename/reorganize renders the new name and folder in
  the File cell, and untouched rows recede. The sel column becomes the per-row
  **apply scope** (every change ticked by default; untick to narrow what a single
  Apply writes), marked by an accent bar on affected rows. A floating
  **Apply / Discard** pill carries the apply count and the plan name — nothing is
  written until Apply, and Discard / Apply / Undo all restore the plain table.
  The `Preview` tab is gone from the strip; `Files` remains. The whole
  preview → apply → undo safety gate is unchanged.

- **Release import moved to a header icon button** (#120). The
  "Import to selected files" action left the tracklist footer (which needed
  scrolling past every track) for a compact, accent-outlined icon — a left arrow
  toward the file table — in the release card's header, beside the expand caret.
  It appears only once the release is loaded and the card is expanded, and stays
  hidden while the tracklist is still being fetched.

- **Duplicate finding is now a top-level DEDUPLICATOR mode** (#118, fourth slice
  of the workspace redesign). The duplicate scan moves out of the
  `Files | Preview | Duplicates` view strip and becomes a mode tab of its own,
  alongside TAGGER / RENAMER / GENERATOR / EXPORTER: its criterion + **Scan
  library** controls sit in the right panel like every other mode, and the
  grouped, **read-only** results (each set badged with its copy count and matched
  key, behind a lock banner) take over the main area using the same table shell.
  The `Duplicates` tab is gone from the strip; `Files | Preview` remain.

- **Mode tabs now read as icon + label** (#116, second slice of the workspace
  redesign). Each top mode (TAGGER / RENAMER / GENERATOR / EXPORTER) gains a 16px
  icon beside its label so the bar is scannable at a glance and the icon carries
  the mode's identity. When the labelled tabs would overflow the bar the app
  drops to a **compact, icon-only** row (tooltip + `aria-label` keep the names) —
  the responsive headroom that keeps a fifth, longer mode from truncating.

- **One inline-SVG icon set, cross-platform native controls** (#115, first slice
  of the workspace redesign). Every unicode/emoji glyph in the chrome
  (`⚙ ⇥ ▶ ■ ✕ ☰ ▦ ▾ ⋮⋮ ▲ ▼`) is replaced by a single 16px inline-SVG set that
  paints in `currentColor`, so icons inherit theme + state and render identically
  across the three Tauri webview engines (WKWebView / WebView2 / WebKitGTK) —
  no more colour-emoji-vs-flat divergence or missing-glyph tofu on Linux. Native
  form chrome is normalized for the same reason: `<select>` drops its
  engine-specific arrow for our own muted caret (the open list stays native, so
  keyboard + screen-reader behaviour is unchanged), the search box hides its
  native clear glyph, and scrollbars render as a consistent thin overlay. Icon-only
  buttons carry `aria-label` + `title`.

- **EDITOR action buttons stay pinned** (#113). **Clear tags** and **Stage field
  changes** used to sit at the very bottom of a fully-scrolling panel, so on open
  they were below the fold. The tag fields and cover well now scroll inside the
  panel while the two actions stay visible as a pinned footer, mirroring the
  ONLINE panel's pinned head.

- Release-card tracklist: the **title · artist** column now sizes to the longest
  row and scrolls horizontally, while the **checkbox + track number** and
  **duration** columns stay pinned either side — so a long track/artist is no
  longer truncated. The middot between title and artist is more visible (was
  nearly invisible). The main file table's rows are also **more compact** (tighter
  row height and a smaller cell font), matching the tracklist's density.

- **Compact List-view release card** (#98, Design pass). The card is denser and
  more image-forward: the cover now fills the header's full height (a square
  driven by the text beside it) instead of a fixed 64px thumbnail; the header is
  three tight lines (catalogue no. + count · album title · album artist +
  country · year · format) instead of four; and the expanded tracklist is a
  tight table — a leading checkbox + track number, a combined **title · artist**
  cell (the per-track artist shows only when it differs from the album artist),
  and a right-aligned duration — with minimal row height. Grid tiles are
  unchanged.

- The window now opens at 1280×800 and can't be shrunk below that, so the layout
  always has room (was 1100×720, min 720×480).

- Aligned the heights of the Files-toolbar controls. The Group dropdown, Filter
  box and Columns button rendered at 28/26/24px because a `<select>`, `<input>`
  and `<button>` have different intrinsic heights; they (and Expand/Collapse) are
  now pinned to a single 28px. The Discard/Apply buttons were already the same
  height — the primary now also carries a defined edge so its solid fill no
  longer reads as taller than the outlined secondary.

- The track/disc count chip on release cards and tiles now uses the same
  monospace font and size as the catalogue-number chip next to it, so the two
  read as a matched pair.

- Release-card polish from testing. **List cards:** the cover is a bit larger
  (56→64px), the expand caret is bigger and highlights on hover/expand so it
  reads as a control rather than a hint, and the redundant "details…" row is
  gone (clicking the card still expands it). The **tracklist** now uses the UI
  (proportional) font with tighter rows, so long track/artist names that the
  monospace clipped ("West Coast Connection", …) now fit. The **search box** uses
  the UI font too (the monospace looked out of place; the RENAMER mask keeps it).
  **Grid tiles** no longer carry the media badge — as an overlay on the full-bleed
  cover it read like the artwork was clipped; disc count and format are already on
  the tile's text lines. (The badge stays on List cards.)

- Restructured the List-view release card into four clear lines (#98): line 1 is
  the catalogue number and track/disc count, line 2 the album artist, line 3 the
  album title, line 4 the rest (country · year · format). Previously the artist
  and title shared one line and the count sat in a separate footer row.

- **Media-type badge on the release cover** (#98, Design pass). A small corner
  badge on the 56px cover shows the medium inferred from the release `format`
  (vinyl / CD / digital / generic glyph) and, for multi-disc sets, the disc
  count (`×2`). It stays legible over both the artwork and the striped
  placeholder. The media glyph shows immediately; the disc count fills in once
  the release is fetched. Applies to both List cards and Grid tiles.

- **Bundled monospace font moved out of the CSS into `assets/tagrex-mono.woff2`**
  (a local bundled asset) instead of a ~33 KB base64 data-URI, shrinking
  `style.css` by a third. The font is still fully offline — no external request.

### Added

- **Regex filtering, scoped queries, and saved presets** (#44). The Files filter
  box gains two in-box toggles: **`.*`** for regular-expression matching and
  **`Aa`** for case sensitivity. A regex is compiled as you type, inside a guard,
  so an invalid pattern just reddens the box (the match is suppressed) instead of
  hanging or emptying the table. A **`field:query`** prefix scopes the search to
  one column — e.g. `artist:aphex`, `title:remix`, `position:B1` — while a bare
  query still searches the file name and every tag value. A new **Presets** popover
  saves the whole view — filter text + both flags, the sort column/direction, and
  the grouping — under a name and re-applies it in one click; presets persist
  across sessions (localStorage).

- **Clear all tags for a fresh start** (#94). A **Clear tags** button in the
  TAGGER → EDITOR panel wipes every text tag from the selected files in one
  step, through the normal preview → apply → **undo** path, so it's reviewable
  and reversible. Only the text metadata TagRex models is cleared — the embedded
  cover and the non-text binary frames the write pipeline preserves (DJ cue
  points/loops, ReplayGain, ratings) are deliberately kept, so it's safe on a DJ
  library. To also drop the cover, use the cover well's Remove.

- **Filter release search by media type** (#103). A selector by the search box
  narrows results to **CD / Vinyl / LP / File** (or All). It maps to the Discogs
  `format` search parameter; MusicBrainz folds it into its `format:` query
  (`File` → `Digital Media`, `LP` → `Vinyl`). Changing it re-runs the search.

- **Media type + vinyl position view.** Release import now writes the **media
  type** (`Vinyl` / `CD` / `Cassette` / `File`) to the standard media tag (ID3v2
  `TMED`, Vorbis `MEDIA`), and there's a **Media** column and a `%media%` rename
  placeholder. Building on the vinyl side→disc mapping, a new **`%side%`** rename
  placeholder renders the disc as a side letter on vinyl/cassette (blank on other
  media), so `%side%%track:1% - %title%` names files `A1 - …` on vinyl and
  `1 - …` on CD from one mask. A derived, read-only **Position** column (toggle it
  in the Columns picker) shows the reconstructed `A1`/`B1` notation; the tags
  themselves stay plain integers.

- **Number tracks** (Generator panel). Fills the track number across the selected
  files in table order without going through a provider import: a start value, an
  optional track total and disc number, and an optional **restart per group**
  when a grouping is active. Existing non-numeric positions (vinyl sides like
  `A1`/`B2`) are left untouched rather than flattened. Staged into the
  pending-edits buffer, so it previews and applies through the usual apply/undo
  path. (Track numbers are stored as plain integers — zero-padding is a file-name
  concern, handled by the RENAMER mask `%track:2%`.)

- **Vinyl sides → disc.** A vinyl-side position (`A1`, a bare side `B`, or the
  reverse `1A`) can't keep its side letter in the integer track-number tag, so
  the side now maps to a **disc number** instead: side A → disc 1, B → disc 2, …
  and the track number restarts per side (`A1 A2 … B` → disc 1 tracks 1,2,… then
  disc 2 track 1). On **release import** an optional "Vinyl side → disc" toggle
  applies this — overwriting a file's default `disc 1` so side B really becomes
  disc 2; a **"Split side → disc"** action in the Generator does the same for
  files already tagged `A1`/`B2` by another tool.

- **Leaner file table.** The per-row Play button column is gone: play a track by
  **double-clicking its file name**, or press the **Play button in the bottom
  bar** to start the current row (the last-clicked / selected one, else the top)
  and auto-advance down the list to the end. The selection-checkbox column is now
  **optional** (Settings › Display, off by default) — rows select on click, with
  Cmd/Shift-click for a range or to toggle — so the file name leads the table.

- **Display preferences** (Settings › Display): a **theme selector**
  (Auto / Light / Dark — Auto follows the system appearance and switches live
  when it changes; Light/Dark force one); an optional **condensed table font**
  (a narrower sans so more of each value fits before it truncates; uses a system
  condensed face until a bundled subset is added); and an **adjustable table font
  size** (10–20px, applied live to whichever face is active). Selected table rows
  also use a stronger tint so the selection is legible on a dense dark table.

- Import now also writes the **total track count** (so a file's track reads as
  `N/total`), the **release country** (to a portable `RELEASECOUNTRY` tag —
  `TXXX:RELEASECOUNTRY` on MP3, the same key in Vorbis/APE — with the full
  country name, e.g. `Belgium`), and the **release webpage URL** (to the ID3v2
  `WOAF` URL frame, so players treat it as a real link, not plain text).

- A new **URL** tag field (backed by the `WOAF` frame). The tag reader/writer
  now handles URL-link frames (stored as a URL locator, not text) rather than
  dropping them, and `%url%` is available as a mask placeholder.

- **Catalogue number** is now surfaced in the UI — a "Catalogue #" column (hidden
  by default, toggle it in the Columns popover) and an editable field in the tag
  editor. The value was already written to the CatalogNumber tag on import; it
  just had nowhere to show before.

- An **Album** option in the search-query presets — fills the search box from the
  selected track's album tag.

- **Reset to default** on the reorderable lists (#91). The **Columns** popover
  has a "Reset to default" footer that restores the default set, order,
  visibility, and widths (File · Artist · Title · Album · Year); **Settings ›
  Read priority** has a "Reset" that restores ID3v2 · Vorbis · APE (applied on
  Save, like the rest of the panel).

- Paged release search with **Load more** and **Stop** (#95, #96). Online search
  now fetches results a page at a time (choose 5/10/15 with the **Show** control)
  instead of a whole page at once, cutting traffic to the provider. A **Load more
  results** button pulls the next batch and appends it; a **Stop** button cancels
  the in-progress background loading once the wanted release is already visible,
  keeping what's shown. Works for both Discogs (`page`/`per_page`) and MusicBrainz
  (`limit`/`offset`).

- Build the search query from a preset, not just manual typing (#97). A selector
  next to the search box fills it from the current selection — **Folder name**,
  **File name** (without extension), or **Artist + Title** — and runs the search;
  typing switches it back to **Manual**.

- Release builds for **x86-64** as well as ARM64. The release workflow builds the
  desktop app on native runners: macOS on Apple Silicon (ARM64), and Windows and
  Linux on both ARM64 and x86-64 — five bundles, each on its own runner since
  Tauri can't be cross-compiled between platforms. (macOS Intel is intentionally
  omitted.)

- Write the label and catalogue number on import (#90). Importing a release now
  fills the **Publisher** (label) and **CatalogNumber** tags — previously the
  catalogue number was shown on the card but never written. A release can list
  several label/catalogue-number pairs (even from one label, e.g. La Bush has
  *AS 5606* and *7243 8 52174 2 5*); when there's more than one, the release card
  shows a small **picker** to choose the single pair to write (default: the
  first). Never concatenated, never put in ISRC. Both Discogs (`labels`) and
  MusicBrainz (`label-info`) supply the pairs.

- Find duplicate tracks (#40). A new read-only **Duplicates** view scans the
  open library and groups likely copies by a chosen criterion — artist+title,
  album+track (both normalized for case/whitespace), identical duration, file
  size, or identical bytes — showing each group with the columns to tell copies
  apart (file, artist/title/album, length, size, bitrate). Detection never
  modifies or deletes anything; any cleanup stays an explicit, separate action.

- Cover art: resize before embedding, and use an external cover file (#41).
  **Resize** — Settings › Cover art adds a max dimension (0 = off) and JPEG
  quality; a larger fetched or chosen cover is downscaled to fit and re-encoded
  as JPEG once, up front, before it reaches the embed path, so it doesn't
  inflate every file. **External cover** — a `cover.jpg` / `folder.jpg` (also
  `.jpeg` / `.png`, and `front.*`) sitting next to the tracks is offered as a
  one-click "Use folder image" in the cover well — the inverse of the sidecar
  export. Both flow through the existing preview/apply/undo path.

- ISRC as an exact match key (#54). When a local file and a provider track carry
  the same ISRC (the per-recording code, ID3 `TSRC`), auto-match treats it as a
  definitive hit — short-circuiting the fuzzy title/artist/duration comparison
  entirely, even when the titles differ — and the match summary calls out how
  many were matched "exact by ISRC". MusicBrainz now fetches per-recording ISRCs
  (`inc=isrcs`) and import writes the ISRC onto files missing one; Discogs
  doesn't expose ISRCs. (ISRC identifies a recording, not a pressing.)

- Camelot / Open Key musical-key notation (#55). GENERATOR gains a **Key
  notation** rule that converts the musical key between Camelot (8A, 5B — what
  harmonic mixing uses), Open Key (1m, 1d), and compact musical (Am, F#) — scope
  it to the new **Key** field to convert a whole set. The converter understands
  sharps/flats (incl. ♯/♭), mode spellings (`m`/`min`/`minor`/`-`, bare or
  `maj`), and already-wheel input, and leaves anything it doesn't recognize
  untouched. (BPM and Key are already modeled tag fields, read/write/sortable as
  columns; this adds the notation conversion. Detecting key/BPM from audio stays
  out of scope.)

- Group the table by release id (#87, finishing the deferred bullet of #20).
  Importing a release now writes its provider id to an album-level tag —
  `MUSICBRAINZ_ALBUMID` for MusicBrainz, `DISCOGS_RELEASE_ID` for Discogs (a
  custom field, so it round-trips as a TXXX frame / Vorbis comment on every
  format) — and the previously-disabled **Group › Release id** option is now
  enabled. Rows cluster by whichever id is present (MusicBrainz UUID first, then
  Discogs integer; they don't collide), with a short `Release <id>…` header.
  Grouping stays a view concern — file order is unchanged.

- MusicBrainz as a second metadata source (#33). TAGGER › ONLINE gains a working
  **Source** dropdown (Discogs / MusicBrainz); MusicBrainz needs no token. A new
  `tagrex-providers-musicbrainz` crate implements the `MetadataProvider`
  boundary with blocking HTTP, a required descriptive User-Agent, and pure,
  fixture-tested response mapping; a free-text search uses MusicBrainz `dismax`
  so a plain "artist album" query matches across fields. Front cover art comes
  from the Cover Art Archive (`coverartarchive.org/release/<mbid>/front`) through
  the existing fetch-and-embed path, and the community genre tags feed the genre
  tag (MusicBrainz has no Discogs-style styles). Requests are spaced to
  MusicBrainz's ~1 req/s etiquette and a 503 is surfaced as rate-limited. The
  candidate list and track-mapping flow are unchanged. (The three provider
  commands are now source-parameterized: `provider_search` /
  `provider_fetch_release` / `provider_fetch_image`.)

- Tag-read priority (#84). Settings › Tag defaults gains a drag-to-reorder
  **Read priority** list (ID3v2 / Vorbis Comments / APE). When a file carries
  more than one tag block, values are read from the highest listed block that is
  present, instead of the tag backend's fixed primary-tag order; a prioritized
  block that isn't present is skipped, falling back to what's there. The order
  persists in `settings.json` and applies on the next library open. Most files
  carry a single block, so the default order is almost always transparent.

- Inline-edit validation in the file table (#76). Editing a typed column
  (year / track / disc / bpm) now validates live with the same rule the EDITOR
  form and backend use: an invalid value lights up the cell's danger-red error
  state, is never staged, and keeps "Preview edits" disabled, so an apply can't
  try to write it. Fixing the value clears the error and stages it as normal.
  (Wires the previously latent `td.error` main-table cell state.)

- Resizable table columns (#76). Every column header has a drag grip on its
  right edge; dragging sets an exact pixel width (the table scrolls horizontally
  when the columns exceed the pane and fills it when they don't), and a
  double-click on the grip resets that column to its default. Widths persist
  across sessions (localStorage) and are keyed by column, so they follow the
  configurable column set (#43). Header clicks still sort; grip drags never do.

- Bundled disambiguating monospace (#76). JetBrains Mono Regular (Apache-2.0),
  subset to Latin + Cyrillic + punctuation and embedded as a data-URI woff2, now
  backs the file table, mask inputs, catalogue numbers and the release
  tracklist. It distinguishes 0/O/o and 1/l/I/i at 11px, covers Cyrillic tags,
  and has its code ligatures stripped so file names never fuse (e.g. "->" stays
  two glyphs). The system mono stack remains the fallback for uncovered glyphs.

- User-configurable table columns (#43). A "Columns" popover in the table
  toolbar lets you show/hide any modeled tag field and drag to reorder them; the
  File column stays pinned first. The header and rows are built from the chosen
  set, inline editing and per-column sorting work for whatever is shown, the
  filter searches every visible field, and the choice persists across sessions
  (localStorage). (Grouping keeps its existing folder/artist/album options.)

- GENERATOR and EXPORTER panels redesigned (Claude Design pass) — the last two
  plain mode panels. **GENERATOR**: the flat rule table becomes drag-reorderable
  rule cards, each a step number + kind + a per-kind body (find/replace with its
  flags, change-case as a segmented control, remove-diacritics header-only).
  Dragging the grip reorders the chain (with a drop indicator and a lifted-card
  state); ↑/↓ stay as the keyboard/fallback. A live empty state keeps the scope
  selector and Add-rule row actionable. **EXPORTER**: Format is now a segmented
  control (Playlist / CSV / Report) with a single hint that swaps per format, a
  Mask row that reveals only for the report format, aligned rows, and a calm
  read-only note.

- A Settings screen (#79, Claude Design pass). A top-bar gear (with a dot when a
  Discogs token is set) opens a right-edge slide-over over a scrim — app-wide
  preferences, deliberately outside the per-mode panel flow. **Discogs**: the
  personal token, promoted here out of the gear behind TAGGER › ONLINE.
  **Network**: an HTTP/SOCKS proxy for Discogs requests, and a client-side rate
  limit (requests/min, 0 = off; the server's 429/Retry-After is honoured either
  way). **Tag defaults**: the ID3v2 version to write (v2.3 or v2.4). Settings
  persist to a `settings.json` in the app config dir and apply immediately.
  (Cover size/quality is deferred to the cover-resize work #41; tag-read
  priority to a later pass.)

- TAGGER › EDITOR redesigned (Claude Design pass). The flat label/input table
  becomes a grouped, scannable form: a **Core** group (Artist, Title, Album,
  Album Artist, Track, Track Total, Disc, Year, Genre) always open and a
  collapsible **Extended** group (Comment, Composer, Publisher, BPM, ISRC, Key,
  custom fields). Typed fields now validate **as you type** — a bad Year, Track,
  Disc, Track Total, or BPM shows an inline error (red row + "✕ 4-digit year" /
  "numbers only" hint) mirroring the backend rule, instead of only being caught
  at apply; numeric fields render as narrow right-aligned figures. The three
  row states — dirty (unsaved), multiple-values (differs across the selection),
  and error — are visually distinct via a left marker, tint, and label
  treatment.

- EDITOR cover art is now a **cover well** instead of two bare buttons. It shows
  the selection's front cover as a thumbnail with three states — one shared
  cover, no cover (the inert-stripe placeholder), or mixed (a small fan of the
  differing covers, "N/M with a cover") — and offers Replace / Remove / Export
  inline. Dragging an image onto the well embeds it (with a drop-target state);
  the whole well is the click/drop target when there's no cover. Remove routes
  through the normal preview/apply/undo path. Backed by two new commands,
  `read_cover_summary` and `preview_cover_remove`.

- Per-field "don't-be-a-fool" validation in the preview. Beyond the year (which
  must be a valid 4-digit timestamp or it corrupts the file), the track, disc,
  and track-total fields now require a plain integer and BPM requires a number
  (integer or decimal), because the tag writer silently drops a non-numeric
  value for those frames. Free-text fields (artist, title, album, genre,
  comment, ISRC, key, catalog number, …) still accept anything. A rejected value
  shows as an error cell in the diff and is skipped on apply. The rule lives in
  the tag engine (`is_writable_value`) so the preview flags exactly what the
  writer would mishandle.

### Added

- Preview rows can be individually excluded from an apply (#81). The change-plan
  diff has a leading checkbox column again (all ticked by default); unticking a
  row drops it from what a single Apply writes and journals, and the header
  checkbox toggles all with an indeterminate state. Excluded tag-edit rows keep
  their staged edits for a later apply.

### Fixed

- A file whose tags can't be parsed is now listed instead of vanishing (#83).
  `list_tracks` used to silently drop any file it couldn't read, so a single
  malformed frame made a track disappear from the library (looking like data
  loss though the file was intact). Such a file now shows as an inert, greyed,
  non-selectable placeholder ("couldn't read tags — file left untouched") that
  still counts in the total; it's kept out of the default selection and every
  mode's preview skips it.

- Auto-match mis-aligned files when some didn't match, which would write wrong
  tags on import. Matched files were packed densely by their matched position,
  so any unmatched file left a gap that shifted every file after it by one —
  and since import maps release track `i` onto file `i` by position, the shifted
  files got another track's title/artist/number. Each matched file is now placed
  at the position of the track it matched, and unmatched files fill the remaining
  slots without displacing the matched ones.

- A short numeric year could corrupt a file and make it vanish from the library.
  The year is written as a timestamp that must be exactly 4 digits; a shorter
  numeric value like `222` was accepted on write but rejected by the tag reader,
  after which the whole file failed to read and was silently dropped from the
  track list (looking like data loss, though the file was intact on disk). The
  year validation now requires exactly 4 digits (matching the tag backend) at
  the preview layer, and `TagEngine::write` guards the year before touching the
  file, so no plan source (edits, transforms, import) can corrupt one — a
  rejected value leaves the file untouched.

### Added

- Preview rejects an invalid tag value instead of writing it (#82). A change
  plan now validates each proposed value and flags a rejected one; the preview
  marks that cell as an error (the state styled in #80/#76) while apply skips it,
  so the field keeps its current on-disk value. The first validated field is the
  year: a non-numeric or implausible year (e.g. `19x6`) is rejected, while a
  plain year or a dated `1996-05-01` passes. The flag rides on the plan DTO
  (`#[serde(default)]`, so older plans still deserialize) and is set at every
  plan source — tag edits, transforms, and Discogs import.

- Preview shown as a table-diff (#80). The staged change plan is no longer a
  vertical `Current → New` list but a table that mirrors the main file table, so
  a batch is scanned in the same layout the user reads it in. The core columns
  (File · Artist · Title · Album · Year) always show; one extra column is added
  per changed non-main field (Album Artist, Track, Genre, …) with an
  accent-underlined header, and a Cover column appears when a cover changes.
  Cells show the new value; unchanged cells are dimmed so changes pop, a folder
  move adds the new path line on the File cell (#37), and the File column stays
  pinned on horizontal scroll. The old value is on hover, or revealed
  struck-through under every changed cell via a "Show old values" toggle. Error
  (rejected value) styling is wired but latent until the backend flags a change
  invalid. (The design's per-row "include in this apply" checkbox is deferred
  until the apply path supports a partial plan.)

- Keyboard row navigation in the file table (#76). A roving focus moves between
  rows with ↑/↓ (drawing the new focus ring), and Space toggles the focused row's
  selection — so a keyboard-heavy tool's main surface is now operable without the
  mouse, complementing the existing click / ⌘ / Shift selection.

- Rich release picker in TAGGER (#27, step 2 of 2). Discogs search results are
  now expandable cards instead of a flat list: each shows a cover thumbnail
  (fetched in the background), the catalogue number as an accent chip, and
  country · year · format. Clicking a card lazily fetches the release and reveals
  its tracklist inline (checkbox · number · title · artist · duration) with a
  live selected count; Enable/Disable all, Auto-match, Embed cover and Import are
  all per-card. A List/Grid toggle switches to compact cover tiles. Backend: the
  Discogs search DTO now carries `thumb_url`, `country`, `label`, `format` and
  `catalog_number`. Nothing is written until Import goes through the usual
  preview/apply/undo path. (The match-confidence bar from the design is deferred
  until candidates are scored against the selection.)

- Native folder picker (#74). A "Browse…" button next to the library path opens
  the OS folder chooser and loads the picked folder (the scanner already
  recurses into its subfolders), so opening a library no longer means pasting a
  path. Built on `tauri-plugin-dialog`; outside the desktop shell it falls back
  to focusing the path field.

- Text transformations (#34): a "Transform…" dialog runs an ordered chain of
  cleanup rules over the selected files' tags or filenames, previewed and applied
  through the normal journaled path. Rules are find-and-replace (literal or
  regex, with whole-word and case-sensitivity switches), change case (lower,
  UPPER, Title, Sentence — with a data-driven exception list that keeps acronyms
  and roman numerals like `DJ` and `III` from being mangled), and remove
  diacritics (`Björk` → `Bjork`, expanding ligatures and `ß`). Rules can be
  reordered and scoped to all tags, one field, or the file name. A malformed
  rule (bad regex, unknown kind) is reported rather than silently doing nothing.

- Conditional sections in masks (#68). `[...]` renders only when a placeholder
  inside it resolved to something and is dropped whole otherwise, so one mask
  serves a library where some albums have a year and some don't:
  `[%albumartist%] - %album%[ (%year%)]/%disc%%track% [%artist% - ]%title%`.
  Sections nest, a missing tag inside one merely suppresses it (outside one it
  is still an error), and `'x'` quotes a literal `%`, `[` or `]`. In the extract
  direction a section becomes an optional group.
- `%catalognumber%` joins the addressable fields, mapped to the catalogue-number
  tag — it appears in real rename patterns and is Discogs' most precise key.

- Track numbers zero-pad to two digits when rendered from a mask (#65), so a
  plain alphabetical sort stays correct and a concatenated `%disc%%track%` reads
  as `101` (disc 1, track 01) rather than `11`, which a player would take for
  track eleven. Any placeholder can set its own width — `%disc:2%`, or
  `%track:1%` to opt out. Values that aren't purely numeric (`A1`, `1/12`) are
  left alone.
- Reorganize files into folders from a template (#37): a "Reorganize…" action
  renders a full relative path from a mask (`%albumartist%/%year% - %album%/
  %track% - %title%`), previews the moves, and applies them through the same
  journaled pipeline as a rename. Missing folders are created, and undo removes
  exactly the folders the batch created — a directory that already existed is
  never deleted, even if the rollback leaves it empty. Only literal slashes in
  the pattern create folders; tag values still have their separators stripped,
  and a pattern that would produce an empty component or climb out of the
  library is refused.
- Extended tag-field editor (#35): a "Fields…" dialog edits every field the
  model knows for the whole selection — album artist, track/disc numbers and
  totals, genre, comment, plus new first-class Composer, Publisher, BPM, ISRC
  and Key fields — and can add arbitrary custom fields. A field whose value
  differs across the selection shows `<multiple values>` and is left untouched
  unless typed into, so editing one field can't silently flatten the rest.
  Changes land in the same pending-edits buffer as inline table edits, and the
  new fields are usable as rename-mask placeholders (%composer%, %bpm%, …).
- Support every container the tag backend handles (#36): AAC, AIFF, WAV, Opus,
  Speex, Musepack, Monkey's Audio and WavPack alongside the original MP3/FLAC/
  OGG/M4A. These files were previously skipped by the scanner even though the
  preview player already decoded them. AIFF, WAV and AAC store their tags in
  ID3v2, so they now take the same concrete-tag write path as MP3 — otherwise
  adding them would have risked exactly the frame loss fixed in #52, and AIFF/WAV
  are where DJ software keeps cue points.
- Use track lengths as a matching signal (#64). Release track durations are now
  parsed and shown in the release view, and folded into the match score:
  agreement confirms, a large gap lowers confidence and is reported as a delta,
  but length never rejects a candidate on its own — provider durations are
  hand-transcribed and disagree with real files too often to be trusted that
  far. Adds order-preserving alignment by duration sequence for the case that
  matters most: a folder of `track01.mp3`-style files with no usable titles,
  where the ordered vector of lengths identifies the release on its own.
- Match provider candidates by content instead of result order (#53). A new
  matching module normalizes titles progressively (case, throwaway attributes
  like "Original Mix", punctuation, leading articles), takes an exact hit at any
  level, and otherwise falls back to a normalized Levenshtein similarity gated
  by a strictness threshold, with optional artist and duration checks. Remix
  credits are never stripped — a remix is a different recording. Discogs search
  results are now ranked by real similarity to the query rather than the order
  the API returned them, and an "Auto-match" action in the release view reorders
  the selected files to line up with the tracklist by title, so an import can no
  longer tag a whole album one title out of step.
- Exporters (#19): an "Export…" action writes the selected tracks into the
  opened library as an extended M3U playlist (relative entry paths and real
  track lengths), a CSV of the tag columns (RFC 4180 quoting), or a text report
  rendered from a mask template using the same placeholders as rename masks.
  Read-only — the audio files are never modified. Export file names must be bare
  names, so an export can't be steered outside the library.
- Expand All / Collapse All for groups (#32): buttons next to the Group selector
  (shown only while grouped) toggle every group at once, reusing the in-place
  collapse path so selection and in-progress edits survive.
- Group the track table (#20): a "Group" selector groups rows by folder, artist,
  or album under collapsible headers showing each group's track count; clicking a
  header collapses/expands it without a re-render, so selection and in-progress
  edits survive. Grouping is strictly a view concern — it never reorders the
  underlying track list, and position-based mapping (rename masks, Discogs
  import) now explicitly follows that list rather than the visual row order.
  Grouping by release id is listed but disabled until a release identifier is
  stored on tracks.
- Gapless playback (#30): the preview player now runs on a native rodio/
  Symphonia backend instead of a WebView `<audio>` element. A dedicated audio
  thread keeps the current and next track queued in one sink, so tracks play
  back-to-back with no gap — seamless on continuous/mixed compilations. As a
  bonus it decodes every format we handle, including OGG (which the old WebView
  player couldn't play). The UI drives it via `player_*` commands and polls a
  status snapshot for the seek bar / time; auto-advance (#29) is now realized by
  the backend queue. CI installs `libasound2-dev` (ALSA) for the Linux build.
- Player auto-advance (#29): when a track finishes, the player automatically
  plays the next visible track (respecting the current sort/filter/manual
  order), continuing down the list until it ends or the user stops. An
  unplayable file (e.g. an unsupported format) is skipped mid-run rather than
  halting playback; a manually chosen unplayable file just reports and stops.
- Always-visible player controls (#31): the player bar stays docked once a
  library is open, showing a disabled idle state ("No track loaded", `0:00 /
  0:00`) instead of appearing only during playback and vanishing on stop.
- Built-in preview player (#28): a ▶ button on each track row auditions the
  file in an in-app player bar (play/pause, stop, seek, elapsed/total time) — no
  external player, no leaving the app. The backend streams the file's bytes to
  the webview as a Blob, which plays the formats WKWebView supports (MP3, M4A,
  FLAC); unsupported files (e.g. OGG) surface a friendly message instead of
  failing silently. Preview-only: reads bytes, never touches the tag pipeline.
- Genre tag from Discogs Style, not Genre (#26): a Discogs import now fills the
  genre tag from the release's `styles` (e.g. `Trance/Tribal/Techno`) rather
  than the coarse `genres` (e.g. `Electronic`), which is closer to what a genre
  tag usually means. Multiple styles are joined with `/` (matching the common
  library convention); releases with no styles fall back to their genres. The
  provider now exposes `genres` and `styles` separately instead of merging them.
- Export embedded cover art to disk (#25): an "Export cover" toolbar action
  saves the embedded front cover of the selected files as a `cover.<ext>`
  sidecar next to each one (extension from the cover's MIME type). Read-only for
  the audio files — it never touches the tag-write/undo path. Tracks sharing a
  folder collapse to a single `cover.jpg` (one write, not one per track), and
  files without an embedded cover are reported as skipped rather than failing.
- Fetch cover art from Discogs (#24): the release detail view now shows the
  release's primary image (downloaded through the backend, since Discogs image
  URLs need the token + User-Agent the webview can't send) with an "Embed
  cover" action that embeds it into the selected files. The fetched bytes reuse
  the same preview/apply/undo cover path as a locally chosen image.
- Cover art embed (#18, core): embed a front cover from a local image file
  into the selected tracks, previewed with a thumbnail and applied through the
  same journaled/undoable path as tags (a new cover change kind in the plan,
  executor, and SQLite journal — undo restores the previous cover). Fetching
  covers from Discogs (#24) and exporting them (#25) are tracked separately.

### Changed

- Reorganized the TAGGER panel into ONLINE / EDITOR sub-tabs (#77). Online Discogs
  search and the release cards live under **ONLINE** with a pinned search header —
  only the results scroll now, not the whole panel; hand-editing tag fields and
  cover art live under **EDITOR**. The Discogs token moved out of the way behind a
  gear toggle (a proper Settings area is still to come) and is remembered as
  before.

- Typography scale and tabular numerals (#76). The ad-hoc font sizes, weights and
  letter-spacings across the UI now reference the design-system type tokens
  (`--text-*`, `--fw-*`, `--ls-*`), tidying the scale without changing how it
  looks. Figures that stack or are compared column-to-column — years, durations,
  track/disc counts, the selection count, player time — now use `tabular-nums`
  so their digits line up. (A bundled disambiguating mono is left as a token slot
  for later; the system mono ships today.)

- Table-row state layering (#76). A row can be hover + selected + dirty (per
  cell) + playing + keyboard-focused at once. Backgrounds are now ranked (dirty
  cell → selected row → hover) and the rest move to orthogonal channels that
  never fight the fill: the **playing** track is a left-edge accent bar (it was a
  full-row tint that overwrote the selection tint), and keyboard focus is an
  inset ring. Also adds latent per-cell error styling for a future rejected-value
  state.

- One "inert / unavailable" visual language (#76). Disabled controls, empty
  states and in-flight loaders now share a single diagonal-stripe motif (the
  release-cover placeholder, generalized): disabled buttons/fields get soft
  stripes under a muted label instead of a flat `opacity` fade, empty states are
  dashed striped panels, and a release's tracklist shows a shimmering skeleton
  while it fetches.

- Visible keyboard focus rings, and the design-system token layer they sit on
  (#76). Every control now shows a two-layer accent focus ring on keyboard
  navigation (`:focus-visible`, so plain mouse clicks stay quiet) — the app is
  keyboard-heavy but previously showed no focus at all. This also lands the
  foundation tokens from the Claude Design pass (type scale, line-heights,
  weights, spacing scale, radii, the focus-ring, the inert-stripe motif, and
  selection/dirty/error tints) that the rest of the states/inert/typography
  integration will build on.

- Split the accent into fill vs ink for text contrast (#76). A new `--accent-ink`
  token carries the accent where it is used as *text* (active tabs, brand mark,
  sort indicator, rule numbers), separate from `--accent` used as a *fill* behind
  white text (buttons, selection, tab underline). This lets the accent-as-text
  clear small-text contrast independently of the fill — the win is in dark mode,
  where the fill green as small text was borderline.

- Reworked the main layout around mode tabs and a persistent file table (#27,
  step 1 of 2). The pile of toolbar buttons that each opened a modal is gone;
  instead the file table is the permanent subject and four mode tabs —
  **RENAMER** (rename mask + reorganize), **TAGGER** (Discogs online + field
  editor + cover), **GENERATOR** (transform/cleanup), **EXPORTER** — swap only a
  right-hand panel. The panel collapses and its divider drags, so the table can
  take the full width. A Files/Preview tab over the table shows every mode's
  change plan in one place (Apply/Discard), and a status line tracks the
  selection count. Selection is now a first-class set that survives re-renders
  (sort, reorder, auto-match, staging edits) instead of living in the DOM and
  being silently reset to "all": click selects a row, ⌘/Ctrl toggles, Shift
  ranges, and double-clicking a group's name toggles that whole group (its caret
  collapses it). Cells edit on double-click; the TAGGER field grid follows the
  current selection. The accent is now the brand green with a dedicated red kept
  for danger (errors, deletions, the "old" side of a diff); the table is a
  compact monospace. Step 2 (a richer release picker with cover thumbnails) is
  tracked on #27. Design follow-ups (colour shades, resizable columns) are on
  #76.

### Fixed

- Undo is scoped to the currently open library (#75). The undo journal is shared
  across every library you open, so after working in one library and opening
  another, the second library's Undo offered the first's batches — and undoing
  one then failed with "path resolves outside the allowed root", stranding it.
  History now lists only batches whose files sit under the open library (matched
  against both the raw and canonicalized root), so Undo always applies to what
  you are actually looking at.

- Discogs disambiguation suffixes are stripped from every artist in a credit,
  not only the last one (#69). Discogs tags each artist individually, so a
  joined credit carries them mid-string — `Zolex (2), Carat Trax (3)`,
  `Oxygen (9) feat. Nbg (2)` — and only the trailing one was removed. Search
  results were worse off: they arrive as one combined "Artist - Title" string
  and were not cleaned at all, so every suffix survived into the candidate list.
  A suffix is now removed wherever it is followed by a name boundary, which
  keeps genuine parentheticals like `Godspeed You! Black Emperor (F#A#)` and
  `Apollo (440) Sound` intact.

- Folder masks respect the platform separator (#71). Only `/` counted as a
  directory boundary, so a pattern written with `\` — the natural form on
  Windows, and what an imported configuration carries — was not recognised as
  having folders at all: the `..` and empty-component guards never saw the
  components, and on macOS the backslash ended up as a literal character inside
  one long file name. Silently doing the wrong thing rather than failing. Both
  separators are now accepted in a pattern, and the path is built component by
  component so the platform supplies its own separator. Tag values keep having
  both stripped, so a value still cannot inject a directory.

- Masks accept two placeholders in a row (#65). `%disc%%track%` was rejected as
  ambiguous at parse time, which also blocked rendering — but only *extraction*
  is ambiguous there, since nothing says where one value ends and the next
  begins. The check moved to `extract`, so such a pattern now renders fine and
  only refuses the filename-to-tags direction.

- Tag writes no longer destroy frames the tag model can't express (#52).
  `TagMap` is text-only, so rebuilding a tag from it wiped everything else on
  every edit, import or rename — DJ cue points and loops, ratings, ReplayGain
  and other private/binary frames. MP3 is now written through its concrete
  ID3v2 tag, because lofty's generic tag doesn't even surface those frames when
  reading, so an MP3 round-tripped through it lost them silently; non-text
  frames are carried over while text frames come from the model, so clearing a
  field still clears it. Cover embed/remove take the same path. Other formats
  keep the generic tag but now start from the file's existing one, not a blank.
- Tag writes no longer strip embedded artwork: `TagEngine::write` rebuilt the
  tag from the text fields only, so any edit/import/rename silently dropped the
  cover. It now carries existing pictures over.

- Sort the track table by column (#21): click a header (File/Artist/Title/
  Album/Year) to sort, click again to reverse; an arrow marks the active
  column. Sorting reorders the underlying list so position-based mapping
  (rename masks, Discogs import) follows the visible order; a manual
  drag-reorder supersedes the column sort.
- Filter the track table (#22): a search box hides rows that don't match a
  substring across the filename and tag columns; the count shows shown/total.
  Filtering is view-only — selection and mapping operate on the visible rows.

- Unified pending-edits model (#23): inline cell edits and Discogs import now
  feed one buffer, so they compose into a single preview and Apply instead of
  two disconnected flows. Import merges into pending edits without overwriting
  a field the user already edited by hand (manual wins), edited/imported
  values both show as dirty cells, and pending tag edits survive a rename
  (remapped to the new paths) rather than being silently lost.

- Discogs release import in the GUI (#10): a Discogs panel (token + search →
  candidate list → release tracklist) imports metadata onto the selected
  files, previewed and applied through the same journaled/undoable path.
  Following the established batch-tagger model, the user resolves the mapping
  explicitly:
  each release track has a checkbox (with Enable/Disable all), and files can
  be drag-reordered in the main table so they line up. Enabled tracks map onto
  the selected files in order; the track number comes from the release track's
  own position (so an aligned file keeps its real number), and files with no
  matching track get only album-level fields. `App::preview_import` builds the
  plan from the user's resolved selection.

### Fixed

- Discogs import no longer scrambles tags: the previous version mapped release
  tracks onto files by scan order (unrelated to the tracklist), silently
  writing wrong artist/title/track to a partial selection. Import is now
  user-resolved (see above), and the track number is never invented from the
  selection index.
- Inline tag editing in the GUI (#9): artist/title/album/year cells in the
  track table are editable; edited cells are highlighted, "Preview edits"
  shows a field-level current→new diff, and Apply writes through the same
  transactional path as renames (so tag edits are journaled and undoable).
  Backed by `App::preview_tag_edits`, which reads each file's current value as
  the change's `old` and drops no-op edits.

- Workspace skeleton: `tagrex-core` (tag model, mask engine, transform
  pipeline, transaction pipeline, undo journal — all module signatures in
  place, `TagEngine` I/O still `todo!()`), `tagrex-providers-discogs`
  (provider trait implementation, HTTP client still `todo!()`), and a
  placeholder `app` binary proving the workspace links.
- CI on GitHub Actions: `cargo fmt --check`, `clippy -D warnings`, `build`,
  `test`.
- Tracking issues for the remaining implementation order from
  `docs/architecture.md` (#1-#7).
- `TagEngine::read`/`write` wired up to `lofty` (#1): the ten first-class
  `TagField`s map to `lofty` `ItemKey`s in both directions, `Custom` fields
  round-trip through `ItemKey::Unknown`.
- `read_tags` example (`cargo run -p tagrex-core --example read_tags --
  <path>`): read-only manual check of what `TagEngine` sees in a real file,
  no GUI required.
- Directory scanner (#2): `scan` walks a tree with `walkdir` and lazily
  yields supported audio files instead of collecting them up front, per the
  50k+ files requirement in `docs/architecture.md`. `scan` example for
  manually checking it against a real library.
- Mask engine (#3): `Mask::parse`/`render`/`extract`. Both directions are
  derived from the same parsed segment list — `render` substitutes it,
  `extract` compiles it into one anchored, escaped regex — so there's no
  second matcher to drift out of sync with `render`. Placeholders are
  limited to the ten first-class `TagField`s for now; `Custom` fields aren't
  addressable from a mask yet.
- Transaction pipeline (#4): `Executor::apply`/`undo` — the only writers in
  the codebase. `apply` takes an `allowed_root` and rejects the whole plan
  (before writing anything) if any path resolves outside it, if the on-disk
  state is stale relative to the plan, or if the plan carries a rename.
  Applied batches are recorded so `undo` can restore each field's previous
  value. `VecJournal`, an in-memory `UndoJournal`, backs the pipeline until
  the persistent SQLite journal (#5) lands. Tag writing and renaming are
  separate operations, each with its own tab as taggers conventionally present
  them; this increment does
  tag writes only, rename tracked separately.
- Persistent SQLite journal (#5): `SqliteJournal` (via `rusqlite`, bundled
  SQLite) durably records batches across three normalized tables so an
  applied batch survives an application restart — the "renamed 8,000 files,
  closed the app, realized the mask was wrong" scenario is now recoverable
  after reopening. Batch ids are assigned by the journal (database
  autoincrement) rather than a process-local counter, so they stay unique
  across restarts.

- Rename execution in `Executor` (#8): a plan's `rename_to` moves are now
  applied (and reversed on undo). Within a file, tags are written before the
  rename so a mid-move failure leaves the file at its old path with new tags;
  undo reverses the move first, then restores tags. The whole plan is
  pre-flighted for rename safety — targets must resolve inside `allowed_root`,
  must not already exist on disk, and two files may not target the same path
  (`PlanError::RenameCollision`). Chained/cyclic renames (a target that is
  another file's source) are conservatively rejected for now.

- Discogs metadata provider (#6): `DiscogsProvider::search`/`fetch_release`
  over a blocking `ureq` client (personal-token auth, required User-Agent).
  429 responses surface as `ProviderError::RateLimited` with the `Retry-After`
  value; auth/not-found/other statuses are mapped too. Discogs' numeric artist
  disambiguation (`Artist (3)`) is stripped through a core transform-pipeline
  step (`StripDiscogsSuffix`). Response mapping is factored into pure functions
  and unit-tested against fixture JSON (no network); a `discogs_search` example
  exercises the live API with a token.

- Application command layer (#7): the `tagrex` crate is a library exposing
  `App` — the thin, GUI-agnostic surface the shell forwards intent to (open
  library, list tracks, preview a mask rename, apply, undo, history, Discogs
  search/fetch). Data crosses the boundary as serde DTOs so `tagrex-core`
  stays serialization-free. The library root doubles as the executor's
  `allowed_root`.
- Tauri 2 desktop shell (#7): the `tagrex` binary is now a Tauri app — a thin
  window whose `#[tauri::command]`s are one-line forwards into `App`, over a
  static HTML/CSS/JS frontend (no npm/JS-framework build step) that renders
  the track table and the current→new rename preview. Verified end to end on
  the real native window: open a folder → preview by mask → apply (real
  renames + a persisted batch in the SQLite journal) → undo (reverted on disk,
  batch cleared). Only the GUI crate needs a modern toolchain (Tauri 2 raises
  the MSRV to 1.82); the core crates stay at 1.75.

### Changed

- `UndoJournal::record` now returns the journal-assigned `BatchId` instead of
  `()`; the journal owns id assignment so ids survive restarts. `TagField`
  gains a lossless `to_storage_key`/`from_storage_key` codec for persistence.

### Fixed

- `TagEngine::read` now also recognizes `RecordingDate` (ID3v2.4 `TDRC`) as
  `TagField::Year`, not just the legacy `Year` (`TYER`). Verified against files
  tagged by the common Windows taggers, which write the year exclusively through
  `RecordingDate` — without this, `Year` was silently empty for most
  real-world files.
