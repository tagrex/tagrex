// English catalogue (#50) — the source of truth.
//
// Every key lives here first: `t` falls back to this catalogue when another
// language has no entry, so a half-translated language shows English rather
// than a raw key, and a key that exists nowhere shows itself (loudly, which is
// how a typo gets noticed).
//
// Keys are `panel.thing`, or `toast.what-happened` for messages. A value is
// either a string, or — when a count decides the wording — an object of plural
// categories (`one`, `few`, `many`, `other`) that `tn` selects from. English
// uses `one` and `other`; other languages use as many as their rules need.
//
// `{name}` in a value is filled from the vars passed to `t`/`tn`; `{n}` is the
// count `tn` was given.
export const en = {
  // ---- units that appear inside other messages ----
  "unit.track": { one: "{n} track", other: "{n} tracks" },
  "unit.file": { one: "{n} file", other: "{n} files" },
  "unit.playlist": { one: "{n} playlist", other: "{n} playlists" },

  // ---- EXPORTER ----
  "exporter.heading": "Export",
  "exporter.format": "Format",
  "exporter.format.playlist": "Playlist",
  "exporter.format.cue": "CUE",
  "exporter.format.csv": "CSV",
  "exporter.format.html": "HTML",
  "exporter.format.xml": "XML",
  "exporter.format.report": "Report",
  "exporter.format.aria": "Export format",
  "exporter.hint.playlist": "An <b>.m3u</b> playlist of the selected tracks, in table order.",
  "exporter.hint.cue": "A <b>.cue</b> sheet — one <b>FILE</b> per track, numbered in table order.",
  "exporter.hint.csv":
    "One <b>row per track</b> with the tag columns — opens in any spreadsheet.",
  "exporter.hint.html":
    "A self-contained <b>HTML table</b> of the tag columns — opens in any browser.",
  "exporter.hint.xml":
    "An <b>XML document</b> — one element per tag, for scripts and other tools.",
  "exporter.hint.report": "Each track rendered through the <b>mask</b> below, one line apiece.",
  "exporter.hint.split": "One <b>.m3u</b> per {grouping}, named by the mask below.",
  "exporter.split": "One per",
  "exporter.split.selection": "Selection",
  "exporter.split.folder": "Folder",
  "exporter.split.album": "Album",
  "exporter.grouping.folder": "folder",
  "exporter.grouping.album": "album",
  "exporter.mask": "Mask",
  "exporter.mask.placeholders": "Placeholders",
  "exporter.mask.placeholdersAria": "Placeholder reference",
  "exporter.name": "File name",
  "exporter.nameMask": "Name mask",
  "exporter.note":
    "<b>Read-only.</b> Written into the opened library folder — your audio files are not modified.",
  "exporter.run": "Export",
  "toast.export.selectFirst": "Select the tracks to export first",
  "toast.export.done": "Exported {tracks} to {file}",
  "toast.export.playlists": "Exported {playlists}",

  // ---- Settings › Display ----
  "settings.language": "Language",
  "settings.language.aria": "Interface language",
  "settings.language.auto": "Auto",
  "settings.language.en": "English",
  "settings.language.ru": "Русский",
  "settings.language.hint":
    "Auto follows your system language. A language the app has no catalogue for falls back to English.",
};
