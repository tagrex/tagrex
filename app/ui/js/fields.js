// The field catalogue (#143 split it out of app.js).
//
// Which tag fields the UI knows about and what they are called: the modeled
// fields in model order, the labels for the custom fields other taggers write,
// and the derived read-only columns. Shared by the table, the column picker,
// grouping and the field editor, which is why it is its own module.
const EXTENDED_FIELDS = [
  ["artist", "Artist"],
  ["title", "Title"],
  ["album", "Album"],
  ["albumartist", "Album Artist"],
  ["track", "Track"],
  ["tracktotal", "Track Total"],
  ["disc", "Disc"],
  ["year", "Year"],
  ["genre", "Genre"],
  ["comment", "Comment"],
  ["composer", "Composer"],
  ["publisher", "Publisher"],
  ["catalognumber", "Catalogue #"],
  ["bpm", "BPM"],
  ["isrc", "ISRC"],
  ["key", "Key"],
  ["url", "URL"],
  ["media", "Media"],
];

// Friendly names for known technical/custom frames (#136). Keyed by the raw
// custom name upper-cased (no "custom:" prefix). A custom key found here is
// promoted into the Standard group with this label; anything else stays a raw
// key/value row in the Advanced group. Extend as new well-known frames show up.
const KNOWN_CUSTOM_LABELS = {
  DISCOGS_RELEASE_ID: "Discogs Release ID",
  MUSICBRAINZ_ALBUMID: "MusicBrainz Album ID",
  MUSICBRAINZ_TRACKID: "MusicBrainz Track ID",
  REPLAYGAIN_TRACK_GAIN: "ReplayGain (track)",
  REPLAYGAIN_TRACK_PEAK: "ReplayGain peak (track)",
  REPLAYGAIN_ALBUM_GAIN: "ReplayGain (album)",
  REPLAYGAIN_ALBUM_PEAK: "ReplayGain peak (album)",
  WWWAUDIOFILE: "Audio file URL",
  ORIGARTIST: "Original Artist",
  ORIGALBUM: "Original Album",
  ORIGYEAR: "Original Year",
  ENCODEDBY: "Encoded by",
  CONDUCTOR: "Conductor",
  LYRICIST: "Lyricist",
  GROUPING: "Grouping",
  SUBTITLE: "Subtitle",
  COPYRIGHT: "Copyright",
  MOOD: "Mood",
  LANGUAGE: "Language",
};

// Virtual (derived, read-only) columns that aren't tag fields (#106). "position"
// reconstructs the vinyl side notation (A1/B2) from media + disc + track.
// Read-only columns derived from something other than a tag: the vinyl side
// notation (#106) and the playing time (#172). They live here rather than in
// EXTENDED_FIELDS because nothing writes them.
const VIRTUAL_COLUMNS = [
  ["position", "Position"],
  ["length", "Length"],
];

// Group key for a track that belongs to no dropped folder (a loose dropped
// file, #127). Not a valid absolute path, so it can't collide with a folder key.
const DROP_LOOSE_KEY = "::loose::";

export { EXTENDED_FIELDS, KNOWN_CUSTOM_LABELS, VIRTUAL_COLUMNS, DROP_LOOSE_KEY };
