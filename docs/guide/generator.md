# GENERATOR

Bulk cleanup. Three blocks that share the panel: a chain of text **transforms**,
**track numbering**, and a **vinyl side** splitter. All three stage a preview.

## Transform

Transforms come as **presets**: named, ordered chains of rules. You make and
change them in the preset editor; everywhere else you only tick them.

**The ticked presets run as part of each job.** Behind the wand button in the
toolbar is a checklist of every preset. What is ticked runs by itself when you
import a release, read tags out of file names, or preview a rename — cleanup as
part of the job, not a second step. The set is shared by every job, and each job
takes only the rules aimed at what it produces:

| Job | Takes the rules on |
| --- | --- |
| Importing a release, FROM NAME | tags |
| RENAMER | the file name and extension |
| GENERATOR | everything |

So a file-name preset ticked for renaming never touches the tags FROM NAME
reads, and a tag preset ticked for imports never renames a file. A row whose
preset has nothing for the job in front of you reads dimmed. The wand button
carries a dot while any ticked rule applies to the current job.

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
  *match case* flags. Whole word is what stops a `Dj` → `DJ` rule from mangling
  `Djibouti`.
- **Change case** — Title, lower, UPPER or Sentence. Title case keeps known
  acronyms and roman numerals intact, so `DJ` doesn't become `Dj` and
  `Symphony III` doesn't become `Symphony Iii`.
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

### The preset editor

**Edit presets…** opens it. Your presets are listed first, the shipped ones
below them.

- **New** starts an empty preset, **Duplicate** copies the open one, **Delete**
  removes one of yours (after asking). Rename a preset in its name field.
- Rules can be dragged into a different order by their grip, and each has an
  on/off tick: a disabled step stays in the preset and contributes nothing, which
  is how you test what one rule is responsible for without deleting it.
- Built-in presets open read-only and stay as shipped. **Duplicate** one to get
  a copy you can change.
- Changes save as you go — when you switch presets, close the editor or quit.

Presets run in list order as a single plan, each seeing what the previous one
did — so one that lower-cases the file name and one that rewrites the extension
compose into one rename instead of the second discarding the first.

### The shipped presets

| Preset | Scope | What it does |
| --- | --- | --- |
| Standard values | all tags | Collapse runs of whitespace and trim the ends |
| Discogs cleanup | all tags | Drop the numeric disambiguator: `Sunbeam (2)` → `Sunbeam` |
| Normalize english | all tags | Title-case, keeping known acronyms and roman numerals |
| General Latin | all tags | Romanize non-Latin scripts, then strip leftover accents |
| No dash | all tags | Dashes to spaces, then collapse the gaps that leaves |
| Lower case | file name | Lower-case the stem, extension untouched |
| File extension | file extension | `.FLAC` → `.flac` |
| FTP format | file name | Plain ASCII, no spaces — a name that survives any server |

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

**Split side → disc** turns a vinyl position into a plain track number plus a
disc number: side A → disc 1, B → disc 2, and so on. `A1` becomes disc 1,
track 1; `B2` becomes disc 2, track 2. The reverse spelling (`1A`) is understood
too.

This exists because a side letter cannot live in an integer track-number tag.
Once split, the side is recoverable for display and for renaming through the
`%side%` [mask placeholder](masks.md#side--vinyl-sides).

Only rows that actually carry a side letter change.
