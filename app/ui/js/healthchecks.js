// The checks behind the library health menu (#436).
//
// Pure: a list of tracks in, the paths each check flags out. No DOM, no imports,
// so it can be read and tested on its own; health.js is the part that draws it.
// Everything is derived from what the table already holds (tags, duration, the
// tag blocks a file carries, and whether any block carries a picture, #447).

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
  { id: "noCover", group: "missing" },
  { id: "titleIsFilename", group: "suspect" },
  { id: "badYear", group: "suspect" },
  { id: "whitespace", group: "suspect" },
  { id: "trackTotal", group: "albums" },
  { id: "dupTrack", group: "albums" },
  { id: "incomplete", group: "albums" },
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

// "3, 7-12": the missing numbers, runs folded, for the menu to show.
function numberRuns(numbers) {
  const runs = [];
  for (const n of numbers) {
    const last = runs[runs.length - 1];
    if (last && n === last[1] + 1) last[1] = n;
    else runs.push([n, n]);
  }
  return runs.map(([a, b]) => (a === b ? `${a}` : `${a}-${b}`)).join(", ");
}

// { sets: { id: Set<path> }, counts: { id: number }, flagged: Set<path>,
//   details: { spelling: [{ field, forms: [{ value, count }] }],
//              incomplete: [{ album, disc, missing, have, total }] } }
//   (`missing` is null for a release counted against its total as a whole)
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
      if (check.group === "missing" && check.field && !value(check.field)) flag(check.id, path);
    }
    // Only a file that was read says so; a track without the flag is not accused.
    if (track.has_cover === false) flag("noCover", path);

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

  const incomplete = [];
  for (const members of albums.values()) {
    // A single track cannot disagree with anything, and a lone "3/12" among
    // unrelated files is not an album anyone is collecting.
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

    // Tracks missing (#534), disc by disc: every number from 1 to the track
    // total no file has, or with no total in the tags, the gaps between the
    // numbers that are there. A disc's total is the largest any of its files
    // states, so one file with the total left off doesn't shorten the album.
    const discs = new Map();
    for (const track of members) {
      const tags = track.tags || {};
      const number = leadingNumber(tags.track);
      if (number === null) continue;
      const disc = leadingNumber(tags.disc) || 1;
      if (!discs.has(disc)) discs.set(disc, { numbers: new Set(), total: null });
      const entry = discs.get(disc);
      entry.numbers.add(number);
      const total = leadingNumber(tags.tracktotal);
      if (total !== null && (entry.total === null || total > entry.total)) entry.total = total;
    }
    let short = false;
    const album = (members[0].tags.album || "").trim();
    // A total can count the whole release rather than one disc: numbers start
    // again at 1 on each disc while every file says "of 50" for 25 + 25. That
    // is when every disc states the same total and it is more than any disc
    // reaches. Then each disc can only be checked for gaps, and the release as
    // a whole against its total, without saying which numbers are gone.
    const entries = [...discs].sort((a, b) => a[0] - b[0]);
    const stated = new Set(entries.map(([, d]) => d.total));
    const releaseTotal =
      entries.length > 1 && stated.size === 1 && !stated.has(null)
        ? [...stated][0]
        : null;
    const wholeRelease =
      releaseTotal !== null && entries.every(([, d]) => Math.max(...d.numbers) < releaseTotal);
    for (const [disc, { numbers: present, total }] of entries) {
      const end = wholeRelease ? Math.max(...present) : (total ?? Math.max(...present));
      const missing = [];
      for (let n = 1; n <= end; n++) if (!present.has(n)) missing.push(n);
      if (!missing.length) continue;
      short = true;
      incomplete.push({
        album,
        disc: discs.size > 1 ? disc : null,
        missing: numberRuns(missing),
        have: [...present].filter((n) => n >= 1 && n <= end).length,
        total: end,
      });
    }
    if (wholeRelease) {
      const have = entries.reduce((sum, [, d]) => sum + d.numbers.size, 0);
      if (have < releaseTotal) {
        short = true;
        incomplete.push({ album, disc: null, missing: null, have, total: releaseTotal });
      }
    }
    if (short) members.forEach((track) => flag("incomplete", track.path));
  }
  incomplete.sort((a, b) => a.album.localeCompare(b.album) || (a.disc ?? 0) - (b.disc ?? 0));

  // Where one value is spelled several ways, every file holding any of the
  // spellings is flagged. No spelling is taken to be the right one: that is not
  // something the tags can say (an album present twice, a name that is
  // legitimately capitalised in one copy), so the check shows what differs
  // instead, in `details`, and leaves the choice to the person.
  const details = [];
  spellings.forEach((byKey, index) => {
    for (const forms of byKey.values()) {
      if (forms.size < 2) continue;
      const list = [...forms].map(([value, entry]) => ({ value, count: entry.count }));
      list.sort((a, b) => b.count - a.count);
      details.push({ field: SPELLING_FIELDS[index], forms: list });
      for (const entry of forms.values()) entry.paths.forEach((path) => flag("spelling", path));
    }
  });
  details.sort(
    (a, b) => b.forms.reduce((n, f) => n + f.count, 0) - a.forms.reduce((n, f) => n + f.count, 0)
  );

  const counts = {};
  const flagged = new Set();
  for (const check of CHECKS) {
    counts[check.id] = sets[check.id].size;
    sets[check.id].forEach((path) => flagged.add(path));
  }
  return { sets, counts, flagged, details: { spelling: details, incomplete } };
}
