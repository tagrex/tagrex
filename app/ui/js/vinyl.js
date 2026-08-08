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

// The side letter for a disc on side-based media (disc 1 → A, …, 26 → Z), else
// null. Mirrors `side_letter_for` in the backend.
function sideLetterOf(media, disc) {
  if (!isSideMedium(media)) return null;
  const d = parseInt(disc, 10);
  return d >= 1 && d <= 26 ? String.fromCharCode(64 + d) : null;
}

// The reconstructed position shown in the Position column: vinyl/cassette get the
// side letter + track ("B" + "3" = "B3"); everything else just the track number.
// Reads pending edits first so it tracks staged changes to media/disc/track.
function vinylPositionOf(track, pending) {
  const get = (key) => (pending && pending.has(key) ? pending.get(key) : track.tags[key] || "");
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

export { isSideMedium, sideLetterOf, vinylPositionOf, parseVinylPosition };
