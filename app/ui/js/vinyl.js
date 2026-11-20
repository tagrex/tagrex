// Vinyl side notation (#105, #106, #143 split it out of app.js).
//
// A side letter is presentation, not storage: the tag holds a disc number, and
// A1/B2 is reconstructed here for the Position column and read back when a
// side-numbered value is parsed. Pure functions over values — no DOM, no state
// — which is why the browser mock can borrow the parser too.

// ---- vinyl side notation view (#106) ----
// Whether a media-type value denotes a side-based medium (vinyl / cassette).
// Mirrors `is_side_medium` in the mask backend.
function isSideMedium(media) {
  const m = (media || "").toLowerCase();
  return ["vinyl", "lp", "shellac", "cassette", "tape", '"', "acetate"].some((k) => m.includes(k));
}

// The medium a provider's free `format` text names (#98): "cassette", "vinyl",
// "cd", "digital" or "generic". First hit wins in that order, case-insensitive.
// Explicit medium names are substrings ("12\" Vinyl", "CDr"); EP and LP are
// whole words and come after CD and file (#803), so "CD, EP" is a CD,
// "File, FLAC, EP" is digital and "Repress" is no record, while a bare "EP" or
// "LP" still reads as one.
function mediaKind(format) {
  const f = (format || "").toLowerCase();
  const has = (...ks) => ks.some((k) => f.includes(k));
  const words = new Set(f.split(/[^a-z0-9]+/));
  const word = (...ks) => ks.some((k) => words.has(k));
  if (has("cassette", "tape")) return "cassette";
  if (has("vinyl", '7"', '10"', '12"', "shellac")) return "vinyl";
  if (has("sacd", "hdcd", "cdr", "compact disc", "cd")) return "cd";
  if (has("file", "flac", "mp3", "wav", "aac", "digital", "download", "streaming")) return "digital";
  if (word("lp", "ep")) return "vinyl";
  return "generic";
}

// The side letter for a disc on side-based media (disc 1 → A, …, 26 → Z), else
// null. Mirrors `side_letter_for` in the backend.
function sideLetterOf(media, disc) {
  if (!isSideMedium(media)) return null;
  const d = parseInt(disc, 10);
  return d >= 1 && d <= 26 ? String.fromCharCode(64 + d) : null;
}

// A stored position that names a side (#796): "A1", a bare "B", a doubled "AA2"
// past side Z or on a double A-side, or the reverse "1A".
const SIDE_POSITION = /^(?:([A-Za-z])\1{0,2}\d*|\d+[A-Za-z])$/;

// The position shown in the Position column. The one the import stored in the
// POSITION field comes first (#796): once a record is one disc (#795), disc +
// track would turn B1 into A3. Without one, vinyl/cassette get the side letter
// + track ("B" + "3" = "B3"); everything else just the track number. Reads
// pending edits first so it tracks staged changes to media/disc/track.
function vinylPositionOf(track, pending) {
  const get = (key) => (pending && pending.has(key) ? pending.get(key) : track.tags[key] || "");
  const stored = String(get("custom:POSITION")).trim();
  if (SIDE_POSITION.test(stored)) return stored;
  const letter = sideLetterOf(get("media"), get("disc"));
  const trackNo = get("track");
  return letter ? letter + trackNo : trackNo;
}

// Rebuild the sortable column headers from `visibleColumns` (the sel + play
// headers are static so #select-all and its listener survive).

// ---- vinyl sides -> disc (#105) ----
// Parse a vinyl-side track value into its side (as a disc ordinal, A=1, B=2, …)
// and its per-side track digits. Handles a bare side ("B"), side-first "A1", and
// the reverse "1A"; any numeric part must be plain digits. Returns
// { disc, track } where `track` is the digit string or null (a bare side has no
// digit — the caller supplies a track number). Returns null for a plain number
// or non-vinyl value. Mirrors `side_disc_from_position` in the backend.
function parseVinylPosition(value) {
  const v = String(value || "").trim();
  if (v.length < 1) return null;
  let side;
  let num;
  if (/[A-Za-z]/.test(v[0]) && /^\d*$/.test(v.slice(1))) {
    side = v[0]; // "B", "A1"
    num = v.slice(1);
  } else if (v.length >= 2 && /[A-Za-z]/.test(v[v.length - 1]) && /^\d+$/.test(v.slice(0, -1))) {
    side = v[v.length - 1]; // reverse "1A", "12B"
    num = v.slice(0, -1);
  } else {
    return null;
  }
  const disc = side.toUpperCase().charCodeAt(0) - 64; // 'A' -> 1
  if (disc < 1 || disc > 26) return null;
  return { disc: String(disc), track: num ? String(parseInt(num, 10)) : null };
}

// ---- vinyl numbering (#795, #797) ----
// The Settings choice that numbers a vinyl release: "records" (the default),
// "position" or "sides". A settings file from before #795 has only the #642
// switch, which keeps its meaning.
function vinylNumberingOf(settings) {
  const s = settings || {};
  return s.import_vinyl_numbering || (s.import_vinyl_position ? "position" : "records");
}

// The side a position names and its digits: "A1" -> { side: "A", digits: "1" },
// "AA" -> { side: "AA", digits: "" }, the reverse "1A" -> { side: "A", digits:
// "1" }. null for anything that names no side ("5", "1-05", "AB", "A1a").
function sideOfPosition(value) {
  const v = String(value || "").trim();
  const front = /^(([A-Za-z])\2{0,2})(\d*)$/.exec(v);
  if (front) return { side: front[1].toUpperCase(), digits: front[3] };
  const back = /^(\d+)([A-Za-z])$/.exec(v);
  return back ? { side: back[2].toUpperCase(), digits: back[1] } : null;
}

// Where each position lands under a numbering other than "position": a
// { disc, track } per position (null for one naming no side) and how many
// discs that makes. Mirrors `vinyl_layout` in the backend: sides ranked by
// first appearance, two to a record, the number running on across a record's
// sides (or per side, from the position's digits, under "sides"). Without a
// release's stated record count a record is always two sides here.
function layoutVinylPositions(positions, numbering) {
  const order = [];
  const counters = new Map();
  const places = positions.map((position) => {
    const parsed = sideOfPosition(position);
    if (!parsed) return null;
    let rank = order.indexOf(parsed.side);
    if (rank < 0) rank = order.push(parsed.side) - 1;
    const disc = numbering === "sides" ? rank + 1 : Math.floor(rank / 2) + 1;
    const n = (counters.get(disc) || 0) + 1;
    counters.set(disc, n);
    const track = numbering === "sides" && parsed.digits ? String(Number(parsed.digits)) : String(n);
    return { disc: String(disc), track };
  });
  const discs = numbering === "sides" ? order.length : Math.ceil(order.length / 2);
  return { places, discs };
}

export {
  isSideMedium,
  mediaKind,
  sideLetterOf,
  vinylPositionOf,
  parseVinylPosition,
  vinylNumberingOf,
  sideOfPosition,
  layoutVinylPositions,
};
