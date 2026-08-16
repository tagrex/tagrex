# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
