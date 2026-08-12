# TAGGER

Tags from two directions. **EDITOR** is you typing; **ONLINE** is a metadata
source filling things in. They are sub-tabs of the same mode because they are
usually used together: pull a release, then fix the three fields it got wrong.

## EDITOR — editing by hand

The field editor shows the tags of the current selection, grouped into **Core**,
**Standard** and **Advanced** so the fields you reach for constantly are not
buried among the ones you touch once a year.

Two rules govern it:

- **Only fields you change are written.** Leaving a field alone leaves the tag
  alone. There is no "write everything" pass that quietly normalizes fields you
  never looked at.
- **A field that differs across the selection shows `<multiple values>`** and is
  left alone unless you type in it. Select an album and set the Genre once, and
  every file gets it; leave the Title showing `<multiple values>` and each file
  keeps its own.

**Stage field changes** turns what you typed into a change plan. As everywhere
else, nothing is written until you Apply it — see
[Change plans, Apply and Undo](change-plans.md).

### Adding a field

**+ Add field** expands into one compact row: a field name and a value. The name
box suggests fields already present in the loaded library, so you don't invent
`ALBUMARTIST` when the rest of the collection says `AlbumArtist`.

### Clear tags

**Clear tags** wipes every text tag from the selected files for a fresh start.
Cover art and DJ cue points are kept, and it stages a preview like everything
else, so it is both gated and undoable. The same action is on the table toolbar.

## Cover art

The cover well shows the artwork of the current selection and adapts to what it
finds:

- **One shared cover** — a thumbnail, and **Replace… / Remove / Export**.
- **Different covers across the selection** — a small fan of thumbnails, a count
  of how many files have one, and **Set one… / Remove all / Export**. It never
  implies a shared image that isn't there.
- **No cover** — a drop target, and an offer to embed a `cover.jpg` or
  `folder.jpg` already sitting next to the tracks if there is one.

You can also **drag an image onto the window** from your file manager: a single
dropped image can only mean one thing, so it is embedded as the cover of the
selection.

**Export** writes the embedded cover next to each file as an image. It is
read-only for your audio, so it writes immediately rather than staging a plan.

Embedding goes through the preview like any other change, and respects the size
and quality limits in [Settings › Cover art](settings.md#cover-art).

## ONLINE — pulling tags from a source

Pick a **Source** — Discogs or MusicBrainz — and search. Discogs needs a personal
token; see [Settings › Discogs](settings.md#discogs).

### Building the query

The dropdown next to the search box builds the query for you from what is
already on disk:

| Preset | Query |
| --- | --- |
| Manual | Whatever you type |
| Folder name | The selected files' folder |
| File name | The selected file's name |
| Album | The Album tag |
| Artist + Title | The Artist and Title tags |

**Folder name** is the workhorse for a collection ripped into
`Artist - Album (Year)` folders.

You can also narrow results by media type — CD, Vinyl, LP, File — and choose how
many results per page (5, 10, 15). **Load more** fetches the next page. A search
in progress can be stopped: the magnifier turns into a stop square, and Escape
also interrupts a sweep, which is what you want the moment the release you were
after is already on screen.

Results show as a **list** or a **grid**; the grid is worth switching to when you
are recognising a release by its sleeve.

### The release card

Click a card's head to expand it and load its tracklist. From there:

- **Auto-match** reorders the selected files to line up with the tracklist,
  matching on content, with ISRC treated as an exact match and called out
  separately in the result ("3 exact by ISRC"). Files that don't match confidently
  keep their place rather than shifting the matched ones — packing matches
  densely would turn one unmatched file into an off-by-one that mis-tags
  everything after it.
- **Embed cover** stages this release's cover onto the selected files.
- **The artwork button** saves the release's images to disk: as `folder.jpg` next
  to the tracks, or all images at once when the release has several.
- **Import** maps the tracklist onto the selected files and stages the result.

Individual tracks in the tracklist can be ticked and unticked, so importing a
compilation where you only want half the tracks doesn't require deselecting
files.

### Label · cat#

A release can list several label and catalogue-number pairs — reissues,
co-releases, sometimes several from one label. Only one pair can be written, so
when there is more than one the card shows a **Label · cat#** selector and you
choose. With a single pair (or none) there is nothing to choose and no selector
appears; import takes the first.

### Vinyl side → disc

The checkbox on the results toolbar controls how a vinyl position is imported. On,
`A1` becomes disc 1 / track 1 and `B2` becomes disc 2 / track 2; off, the position
is taken as given. Side letters cannot live in an integer track-number tag, which
is why this mapping exists at all — see the `%side%` placeholder in the
[mask reference](masks.md).

## Import, then fix

Import stages a plan; it does not write. This is the moment to read what the
source is about to give you — providers disagree about artist naming, about
whether "feat." belongs in the title, about genre vocabulary. Untick the rows you
don't want, apply the rest, then clean up the difference with
[GENERATOR](generator.md).

For a disagreement you have every time rather than once, **Settings › Online
import** lists every tag an import can write, each with a checkbox. Switch off
the ones you curate yourself — genre is the usual one — and an import leaves
them exactly as the file has them, instead of writing them and making you undo
it. Everything is on by default, and a field added to the import in a later
version arrives switched on too.

Cover art is not in that list: embedding one is its own button on the release
card, so it is already something you ask for rather than something an import
does on its own.
