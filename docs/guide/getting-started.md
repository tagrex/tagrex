# Getting started

A first pass through the whole loop: open some files, change one thing, see the
change before it happens, apply it, undo it. Ten minutes, and nothing is at risk
— the last step puts everything back.

## 1. Open some music

Three ways in, all equivalent:

- Type or paste a folder path into the bar at the top and press **Open**.
- Press **Browse…** and pick a folder.
- Drag a folder from your file manager onto the window.

Dragging is the most flexible: a single folder opens as a library rooted there,
while a mix of loose files and several folders opens as a *file set* — the
dropped folders become table groups, and anything not under one of them collects
under "Files".

Supported formats: MP3, FLAC, Ogg Vorbis, Opus, Speex, M4A/MP4, AAC, AIFF, WAV,
Musepack, Monkey's Audio, WavPack. Anything else in the folder is ignored.

> **Work on copies the first time.** Not because TagRex is careless with files —
> every change is previewed and journaled — but because the fastest way to trust
> a tool is to watch it do something reversible to files you do not care about.

## 2. Look before you touch

The table fills with one row per file. Click a row to select it; the panel on the
right is TAGGER, showing that file's tags.

Two habits worth forming now:

- **Widen the table.** The button at the top left of the mode panel collapses it,
  giving the table the whole window. The splitter between them also drags.
- **Add the columns you actually care about.** **Columns** in the toolbar picks
  which tag fields are shown and in what order. The defaults are deliberately few.

## 3. Change one thing

Double-click a cell in the table — say a **Title** — and type. The cell marks
itself as edited, and **Preview edits** in the status bar at the bottom lights up.

Nothing has been written. The edit is pending, sitting in the app, and you can
keep making more.

## 4. See the change before it happens

Press **Preview edits**.

The table turns into a diff. Changed cells show their new value; a bar floats
over the table saying how many files are about to change, with **Show old
values** to reveal what each cell is being replaced with, and **Discard** /
**Apply**.

The checkbox column that appears is not a selection — it is the *apply scope*.
Untick a row and it is left out when you apply. This is the moment to disagree
with the tool.

## 5. Apply

Press **Apply**. Now it is written, and the table returns to normal.

## 6. Undo it

Press the undo arrow in the top bar. The batch is rolled back — every file in it,
as one unit.

The undo journal is a database on disk, not a list in memory: close TagRex,
reopen it, open the same library, and the undo is still there waiting. That is
what makes it safe to apply a change to four hundred files at the end of a long
evening.

## Where to go next

- The table is where you will spend your time: [The file table](file-table.md).
- The preview gate above is worth understanding properly, including what it does
  *not* cover: [Change plans, Apply and Undo](change-plans.md).
- To fill in tags from an online source rather than by hand: [TAGGER](tagger.md).
