# GENERATOR

Bulk cleanup. Three blocks that share the panel: a chain of text **transforms**,
**track numbering**, and a **vinyl side** splitter. All three stage a preview.

## Transform

Transforms come as **presets**: named, ordered chains of rules. You make and
change them in the preset editor; everywhere else you only tick them.

**The ticked presets run as part of each job.** Behind the wand button in the
toolbar is a checklist of every preset. What is ticked runs by itself when you
import a release, read tags out of file names, or preview a rename — cleanup as
part of the job, not a second step. Each job keeps its own ticked set, and takes
only the rules aimed at what it produces:

| Job | Its own set | Takes the rules on |
| --- | --- | --- |
| Importing a release (ONLINE) | Import | tags |
| FROM NAME, FROM TEXT | From name and text | tags |
| RENAMER and its mask builder | Renaming | the file name and extension |
| GENERATOR | GENERATOR | everything |

So title case ticked to tidy names read out of files does not run on releases
imported from a source, where the values are already right, and a file-name
preset ticked for renaming never touches tags. The checklist shows the set of
the job in front of you; a preset with nothing for that job reads dimmed and
can't be ticked there. The wand button carries a dot while any ticked rule
applies to the current job.

In GENERATOR the checklist sits in the panel, and **Preview changes** runs every
ticked preset over the selection — or over a staged plan, revising it rather
than adding a second one.

### Scope — what the rules act on

**Apply to** decides what the chain sees:

| Scope | Acts on |
| --- | --- |
| All tag fields | Every text field the file carries |
| A single field | Artist, Title, Album, Album Artist, Genre, Comment or Key alone |
| File name | The file name's stem — the extension is untouched |
| File extension | The extension alone — the stem is untouched |

The two name scopes produce a rename plan; the tag scopes produce an edit. Either
way it applies and undoes through the normal path.

`File extension` refuses a result containing a separator or a dot — that would
move the file or change how many extensions the name has — and skips a file with
no extension rather than giving it one.

### Rule kinds

- **Find and replace** — literal or regular-expression, with *whole word* and
  *match case* flags, the `.*`, `Aa` and `ab` buttons inside the find field.
  Whole word is what stops a `Dj` → `DJ` rule from mangling `Djibouti`.
- **Change case** — Title, lower, UPPER or Sentence. Title case keeps known
  acronyms and roman numerals intact, so `DJ` doesn't become `Dj` and
  `Symphony III` doesn't become `Symphony Iii`. **Keep small words lowercase**
  leaves articles, short prepositions and conjunctions in lower case inside a
  title, `Lord of the Rings` rather than `Lord Of The Rings`, unless one starts
  or ends the title or a part of it (after a spaced dash, a colon or a slash,
  or inside brackets): `Lord of the Rings - The Return of the King`. Pick the
  list by language (English, German, Spanish, French or Italian) and edit the
  words as text; nothing guesses the language of a value. Off by default.
- **Remove diacritics** — strips accents off Latin letters: `Björk` → `Bjork`.
- **Transliterate to Latin** — maps a whole non-Latin script onto Latin:
  `Пётр` → `Pyotr`, `Ελλάδα` → `Ellada`. Cyrillic and Greek. This is a different
  job from removing diacritics, and Latin text passes through untouched.
- **Transliterate to Cyrillic** — the reverse, for tags that arrived already
  romanized. Reversing a romanization is guesswork, so it is built to be wrong
  as rarely as possible: longest run first (`shch` → щ before `sh` → ш), and a
  word containing a letter with no Cyrillic reading is left in Latin entirely,
  which is what keeps `Jazz` and `The` from becoming `Jазз` and `Тхе`. What the
  forward direction discarded stays discarded: `ъ`/`ь` romanize to nothing, and
  `й`/`ы` both come back as `й`.
- **Key notation** — converts a musical key between musical, Camelot and Open Key
  notation. Best pointed at the Key field; unrecognized values are left alone.

### Field rules

The rules above rewrite one value from itself. These work on the file's fields
together, in the same ordered pass: a field rule sees what the rules before it
left, and the rules after it see what it set.

- **Set field from a mask**: the target field gets what a
  [mask](masks.md) renders from the file's current values, so
  `$if2(%albumartist%,%artist%)` fills a missing album artist from the artist
  and `%publisher% %catalognumber%` builds a comment. A placeholder the file
  has no value for is empty rather than an error, nothing is made safe for a
  file name (a `/` stays a `/`), and a track number pads only to a width the
  mask states (`%track:2%`). An empty result removes the field. **Custom
  field…** in the target list sets a field the app does not model.
- **Extract from a field**: renders the first mask (usually one field, like
  `%title%`) and reads fields out of the result with the second, the way FROM
  NAME reads a file name: `%artist% - %title%` takes the artist out of a title
  that carries it. A file the mask does not fit is left as it is, and the
  preview says how many there were.
- **Remove fields**: removes the fields listed, separated by `;`. Names can be
  written as the app shows them or as tag frames name them: `Album Artist`,
  `TRACKNUMBER`, `LABEL`, and `custom:NAME` for a custom field.
- **Keep only fields**: removes every text field not listed. Cover art and the
  binary frames (DJ cue points, ReplayGain) stay, as with Clear tags.

For the fields written as several values (artist, album artist, genre,
composer):

- **Split values** turns a value holding a delimiter (` & `, `, `, ` feat. `)
  into separate values.
- **Merge values** joins the values into one with the text you give.
- **Remove repeated values** keeps the first of each value, in order.

Field rules run in GENERATOR and in the cleanup after an import, FROM NAME and
FROM TEXT, where they read the file's values with the staged ones laid over
them. There the other rules still clean only the values the job produced and
the ones a field rule set, never the rest of the file. EDITOR leaves field
rules out: the values typed there belong to no file yet. A rule aimed at a
locked field leaves it alone and the preview reports the lock, and nothing
later in the chain reads the value it was refused.

### The preset editor

**Edit presets…** opens it, and the › at the end of a preset's row in the
checklist opens it on that preset. Your presets are listed first, the shipped
ones below them.

- **New** starts an empty preset, **Duplicate** copies the open one, **Delete**
  removes one of yours (after asking). Rename a preset in its name field.
- Rules can be dragged into a different order by their grip, and each has an
  on/off checkbox in its head: a disabled step stays in the preset, drawn with a
  dashed edge, and contributes nothing, which is how you test what one rule is
  responsible for without deleting it.
- Built-in presets open read-only and stay as shipped, shown as what they do
  rather than as a form. **Duplicate** under the list gives a copy you can
  edit.
- Changes save as you go — when you switch presets, close the editor or quit.

Presets run in list order as a single plan, each seeing what the previous one
did — so one that lower-cases the file name and one that rewrites the extension
compose into one rename instead of the second discarding the first.

### The shipped presets

| Preset | Scope | What it does |
| --- | --- | --- |
| Standard values | all tags | Collapse runs of whitespace and trim the ends |
| Discogs cleanup | all tags; artists | Drop the numeric disambiguator: `Sunbeam (2)` → `Sunbeam`; on the artist and album artist, also the name-variation asterisk a release page shows: `NBG*` → `NBG` |
| Normalize english | all tags | Title-case, keeping known acronyms and roman numerals |
| General Latin | all tags | Romanize non-Latin scripts, then strip leftover accents |
| No dash | all tags | Dashes to spaces, then collapse the gaps that leaves |
| No underscore | all tags | Underscores to spaces, then collapse the gaps that leaves |
| Lower case | file name | Lower-case the stem, extension untouched |
| File extension | file extension | `.FLAC` → `.flac` |
| FTP format | file name | Plain ASCII, no spaces or dots (the extension keeps its own) — a name that survives any server |

Every one of these is a chain you could have built by hand; none of them needs a
rule kind that isn't in the list above.

## Number tracks

Fills track numbers across the selected files **in table order** — so sort the
table the way you want them numbered first.

- **Start at** — the first number. Usually 1.
- **Disc #** — also write a disc number; leave blank to skip.
- **track total** — also write the count of numbered files to every file.
- **restart per group** — with grouping on, start over at each group. This is how
  you number a multi-disc set grouped by folder in one pass.
- **keep vinyl sides** — leave non-numeric positions like `A1` and `B2` alone
  instead of renumbering them. On by default.

## Vinyl sides

**Vinyl sides** numbers the selected files from their vinyl positions, the way
**Settings › Online import › Vinyl numbering** says. A file's position is its
track number when that names a side (`A1`, `B2`, the reverse `1A`, a bare `B`),
and otherwise the `POSITION` field an import stored, so a vinyl release that
came in numbered `1, 2, 1, 2` on one disc can be repaired here.

- **By record**: each record is a disc (two sides each) and the track number
  runs on across both sides: `A1`, `A2`, `B1` become disc 1 tracks 1, 2, 3.
- **Side = disc**: side A is disc 1, B disc 2, and `B2` is track 2.
- **Position as is**: the position goes into the track number (not on an MP4
  file, which can only hold a number).

The block numbers from the selection in its order, and the disc total counts
the records or sides it found, so select the whole release. The position stays
in `POSITION`, where the `%position%` and `%side%`
[mask placeholders](masks.md#side--vinyl-sides) read it. Only rows that name a
side change.
