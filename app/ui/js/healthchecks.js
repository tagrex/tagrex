// The checks behind the library health menu (#436).
//
// Pure: a list of tracks in, the paths each check flags out. No DOM, no imports,
// so it can be read and tested on its own; health.js is the part that draws it.
// Everything is derived from what the table already holds (tags, duration, the
// tag blocks a file carries), which is why there is no cover check yet: whether a
// file has a picture is not in the track list and would need a read per file.

// id: what the menu and the tests call the check. group: the menu section.
// field: the tag a "missing" check is about.
export const CHECKS = [
  { id: "missing:title", group: "missing", field: "title" },
  { id: "missing:artist", group: "missing", field: "artist" },
  { id: "missing:album", group: "missing", field: "album" },
  { id: "missing:albumartist", group: "missing", field: "albumartist" },
  { id: "missing:track", group: "missing", field: "track" },
  { id: "missing:year", group: "missing", field: "year" },
  { id: "missing:genre", group: "missing", field: "genre" },
  { id: "titleIsFilename", group: "suspect" },
  { id: "badYear", group: "suspect" },
  { id: "whitespace", group: "suspect" },
  { id: "trackTotal", group: "albums" },
  { id: "dupTrack", group: "albums" },
  { id: "albumDisagree", group: "albums" },
  { id: "spelling", group: "library" },
  { id: "unreadable", group: "files" },
  { id: "multiBlock", group: "files" },
];

// The tags whose spelling is compared across the library.
const SPELLING_FIELDS = ["artist", "albumartist", "album", "genre"];

// Case, accents and runs of spaces do not make two spellings different people.
function normalize(text) {
  return text
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/\s+/g, " ")
    .trim();
}

function splitPath(path) {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return { dir: path.slice(0, cut + 1), name: path.slice(cut + 1) };
}

function stemOf(name) {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

// The number in "3" or "3/12".
function leadingNumber(text) {
  const match = /^\s*(\d+)/.exec(text || "");
  return match ? Number(match[1]) : null;
}

const YEAR_SHAPE = /^\d{4}([-/.]\d{1,2}([-/.]\d{1,2})?)?$/;

function yearIsOdd(text, currentYear) {
  if (!YEAR_SHAPE.test(text)) return true;
  const year = Number(text.slice(0, 4));
  return year < 1900 || year > currentYear + 1;
}

// { sets: { id: Set<path> }, counts: { id: number }, flagged: Set<path> }
export function computeHealth(tracks, currentYear = new Date().getFullYear()) {
  const sets = {};
  for (const check of CHECKS) sets[check.id] = new Set();
  const flag = (id, path) => sets[id].add(path);

  const albums = new Map();
  const spellings = SPELLING_FIELDS.map(() => new Map());

  for (const track of tracks) {
    const { path } = track;
    if (track.unreadable) {
      flag("unreadable", path);
      continue;
    }
    const tags = track.tags || {};
    const value = (key) => (tags[key] || "").trim();
    const { dir, name } = splitPath(path);

    for (const check of CHECKS) {
      if (check.group === "missing" && !value(check.field)) flag(check.id, path);
    }

    const title = value("title");
    if (title) {
      const stem = normalize(stemOf(name));
      const unnumbered = normalize(stemOf(name).replace(/^\d+[\s._-]+/, ""));
      const wanted = normalize(title);
      if (wanted === stem || wanted === unnumbered) flag("titleIsFilename", path);
    }
    const year = value("year");
    if (year && yearIsOdd(year, currentYear)) flag("badYear", path);
    if (Object.values(tags).some((text) => typeof text === "string" && text !== text.trim())) {
      flag("whitespace", path);
    }
    if ((track.tag_blocks || []).length > 1) flag("multiBlock", path);
    // Track 9 of 5 is wrong on its own, album or no album.
    const number = leadingNumber(tags.track);
    const total = leadingNumber(tags.tracktotal);
    if (number !== null && total !== null && number > total) flag("trackTotal", path);

    const album = normalize(tags.album || "");
    if (album) {
      const key = `${dir}\u0000${album}`;
      if (!albums.has(key)) albums.set(key, []);
      albums.get(key).push(track);
    }
    SPELLING_FIELDS.forEach((field, index) => {
      const raw = value(field);
      if (!raw) return;
      const key = normalize(raw);
      const forms = spellings[index].get(key) || new Map();
      const entry = forms.get(raw) || { count: 0, paths: [] };
      entry.count += 1;
      entry.paths.push(path);
      forms.set(raw, entry);
      spellings[index].set(key, forms);
    });
  }

  for (const members of albums.values()) {
    // A single track cannot disagree with anything.
    if (members.length < 2) continue;
    const totals = new Set();
    const years = new Set();
    const albumArtists = new Set();
    const numbers = new Map();
    for (const track of members) {
      const tags = track.tags || {};
      const total = leadingNumber(tags.tracktotal);
      if (total !== null) totals.add(total);
      const number = leadingNumber(tags.track);
      const year = (tags.year || "").trim().slice(0, 4);
      if (year) years.add(year);
      const albumArtist = normalize(tags.albumartist || "");
      if (albumArtist) albumArtists.add(albumArtist);
      if (number !== null) {
        const slot = `${leadingNumber(tags.disc) || 1}:${number}`;
        if (!numbers.has(slot)) numbers.set(slot, []);
        numbers.get(slot).push(track.path);
      }
    }
    if (totals.size > 1) members.forEach((track) => flag("trackTotal", track.path));
    if (years.size > 1 || albumArtists.size > 1) {
      members.forEach((track) => flag("albumDisagree", track.path));
    }
    for (const paths of numbers.values()) {
      if (paths.length > 1) paths.forEach((path) => flag("dupTrack", path));
    }
  }

  // Where one value is spelled several ways, the commonest spelling is taken to
  // be the intended one and the files holding any other are what gets flagged.
  for (const byKey of spellings) {
    for (const forms of byKey.values()) {
      if (forms.size < 2) continue;
      let keep = null;
      for (const [raw, entry] of forms) {
        if (keep === null || entry.count > forms.get(keep).count) keep = raw;
      }
      for (const [raw, entry] of forms) {
        if (raw !== keep) entry.paths.forEach((path) => flag("spelling", path));
      }
    }
  }

  const counts = {};
  const flagged = new Set();
  for (const check of CHECKS) {
    counts[check.id] = sets[check.id].size;
    sets[check.id].forEach((path) => flagged.add(path));
  }
  return { sets, counts, flagged };
}
