// The browser-only fake of the Rust command layer (#143 split it out of app.js).
//
// Loaded in every build but reached only when `window.__TAURI__` is absent —
// that is, when the UI is served as plain files to develop or verify it without
// the native shell. It keeps its own in-memory library and answers every
// command the frontend sends, closely enough to drive the interface. It is NOT
// a second implementation of the backend: anything subtle has to be checked in
// the real app.
//
// It reaches for two things the real UI owns — a file-name helper and the
// vinyl-position parser — because it fakes what the backend derives from them.
import { fileName } from "./dom.js";
import { parseVinylPosition } from "./vinyl.js";

// Browser-only fake of the native player: a wall-clock timer advances position,
// auto-advances to the queued `next` on end, and reports status — enough to
// exercise the polling/gapless-feed UI without the rodio backend. Uses a short
// fixed duration so transitions are quick to observe.
const mockPlayer = {
  current: null,
  next: null,
  duration: 600, // seconds (long, so tests aren't raced by natural track end)
  base: 0, // position at last (re)start
  started: 0, // Date.now() when the current run began
  pausedAt: 0, // Date.now() when paused, else 0
  position() {
    if (!this.current) return 0;
    const now = this.pausedAt || Date.now();
    return this.base + (now - this.started) / 1000;
  },
  restart(base = 0) {
    this.base = base;
    this.started = Date.now();
    this.pausedAt = 0;
  },
  status() {
    if (this.current) {
      // Advance across the (gapless) boundary when the current track ends.
      if (this.position() >= this.duration) {
        if (this.next) {
          this.current = this.next;
          this.next = null;
          this.restart(0);
        } else {
          this.current = null;
        }
      }
    }
    return {
      path: this.current,
      is_paused: !!this.pausedAt,
      position_secs: this.current ? Math.min(this.position(), this.duration) : 0,
      duration_secs: this.current ? this.duration : 0,
      // Mirrors the backend's PRIME_LEAD_SECS gate: the queue is primed near
      // the END of the track, not at its start, so a Repeat change mid-track
      // still decides what plays next.
      wants_next: !!this.current && !this.next && this.duration - this.position() <= 5,
    };
  },
};

// Compact Camelot/Open Key/musical converter — the browser-only mirror of the
// backend KeyNotation step, so the transform preview shows real conversions.
// A representative Cyrillic/Greek transliteration for the dev mock only (#72);
// the authoritative, complete table lives in Rust (transform.rs). Enough to see
// the pipeline work in the Browser pane ("Пётр" -> "Pyotr").
function mockTransliterate(value) {
  const MAP = {
    а: "a", б: "b", в: "v", г: "g", д: "d", е: "e", ё: "yo", ж: "zh", з: "z",
    и: "i", й: "y", к: "k", л: "l", м: "m", н: "n", о: "o", п: "p", р: "r",
    с: "s", т: "t", у: "u", ф: "f", х: "kh", ц: "ts", ч: "ch", ш: "sh",
    щ: "shch", ъ: "", ы: "y", ь: "", э: "e", ю: "yu", я: "ya",
    α: "a", β: "v", γ: "g", δ: "d", ε: "e", θ: "th", λ: "l", ς: "s", σ: "s", ω: "o",
  };
  return [...String(value)]
    .map((ch) => {
      const lower = ch.toLowerCase();
      const mapped = MAP[lower];
      if (mapped == null) return ch;
      return ch === lower || mapped === "" ? mapped : mapped[0].toUpperCase() + mapped.slice(1);
    })
    .join("");
}

// The reverse direction for the dev mock only (#137) — the authoritative table
// and the per-word guard live in Rust (transform.rs). Same shape: longest run
// first, and a word holding a letter with no Cyrillic reading is left alone.
function mockUntransliterate(value) {
  const RUNS = [
    ["shch", "щ"], ["yo", "ё"], ["yu", "ю"], ["ya", "я"], ["zh", "ж"], ["kh", "х"],
    ["ts", "ц"], ["ch", "ч"], ["sh", "ш"], ["a", "а"], ["b", "б"], ["v", "в"],
    ["g", "г"], ["d", "д"], ["e", "е"], ["z", "з"], ["i", "и"], ["y", "й"],
    ["k", "к"], ["l", "л"], ["m", "м"], ["n", "н"], ["o", "о"], ["p", "п"],
    ["r", "р"], ["s", "с"], ["t", "т"], ["u", "у"], ["f", "ф"],
  ];
  return String(value).replace(/[\p{L}\p{N}']+/gu, (word) => {
    const lower = word.toLowerCase();
    let out = "";
    for (let at = 0; at < word.length; ) {
      if (!/[a-z]/.test(lower[at])) {
        out += word[at++];
        continue;
      }
      const run = RUNS.find(([latin]) => lower.startsWith(latin, at));
      if (!run) return word; // no Cyrillic reading — keep the word whole
      out += word[at] === lower[at] ? run[1] : run[1].toUpperCase();
      at += run[0].length;
    }
    return out;
  });
}

// The string a FROM NAME mask is matched against (#139), for the dev mock: the
// stem plus one parent folder per separator in the pattern.
function mockNameSubject(path, mask) {
  const depth = (mask.match(/[/\\]/g) || []).length;
  const parts = path.split("/");
  const stem = (parts.pop() || "").replace(/\.[^.]*$/, "");
  return [...parts.slice(Math.max(0, parts.length - depth)), stem].join("/");
}

// Mask -> tags for the dev mock only. Plain placeholders, stated widths (#140)
// and %skip%, no conditional sections: enough to drive the panel in a browser.
// The real bidirectional grammar is mask.rs, and this is not a second
// implementation of it — anything subtle must be checked in the native app.
const MOCK_INTEGER_FIELDS = ["track", "tracktotal", "disc", "disctotal"];
function mockExtractFromName(mask, subject, cleanup) {
  const fields = [];
  let pattern = "^";
  for (const part of mask.replace(/\\/g, "/").split(/(%[a-z]+(?::\d+)?%)/i)) {
    if (!part) continue;
    const placeholder = /^%([a-z]+)(?::(\d+))?%$/i.exec(part);
    if (!placeholder) {
      pattern += part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      continue;
    }
    const field = placeholder[1].toLowerCase();
    const width = placeholder[2] ? +placeholder[2] : 0;
    if (field === "skip") {
      pattern += "(?:.+?)";
      continue;
    }
    fields.push(field);
    // A stated width is a fixed length, digits on the integer fields.
    pattern += width
      ? `(${MOCK_INTEGER_FIELDS.includes(field) ? "\\d" : "."}{${width}})`
      : "(.+?)";
  }
  const match = new RegExp(pattern + "$").exec(subject);
  if (!match) return null;
  // The integer fields are stored as numbers, so the backend writes a name's
  // "05" as "5"; normalize here too or the mock shows a value the app wouldn't.
  const numeric = ["track", "tracktotal", "disc", "disctotal"];
  return fields.map((field, i) => {
    // Cleanup first, then trim/normalize — the order the backend uses. A group
    // acts on the value its scope names, "tags" on every one (#144).
    const cleaned = (cleanup || []).reduce(
      (acc, g) => (g.scope === "tags" || g.scope === field ? mockApplyRules(acc, g.rules) : acc),
      match[i + 1],
    );
    const value = cleaned.trim();
    return [field, numeric.includes(field) && /^\d+$/.test(value) ? String(+value) : value];
  });
}

// One chain over one value, for the dev mock only — shared by the single-chain
// preview and the checklist run so the two can't drift apart.
function mockApplyRules(value, rules) {
  let out = value;
  for (const rule of rules || []) {
    if (rule.enabled === false) continue; // disabled step (#57)
    if (rule.kind === "replace" && rule.from) {
      out = rule.regex
        ? out.replace(new RegExp(rule.from, rule.case_sensitive ? "g" : "gi"), rule.to)
        : out.split(rule.from).join(rule.to);
    } else if (rule.kind === "case" && rule.style === "title") {
      out = out.replace(/[\p{L}\p{N}']+/gu, (w) => w[0].toUpperCase() + w.slice(1).toLowerCase());
    } else if (rule.kind === "case" && rule.style === "lower") {
      out = out.toLowerCase();
    } else if (rule.kind === "case" && rule.style === "upper") {
      out = out.toUpperCase();
    } else if (rule.kind === "key") {
      out = mockKeyNotation(out, rule.style);
    } else if (rule.kind === "transliterate") {
      out = mockTransliterate(out);
    } else if (rule.kind === "untransliterate") {
      out = mockUntransliterate(out);
    }
  }
  return out;
}

function mockKeyNotation(value, style) {
  const MAJOR = [8, 3, 10, 5, 12, 7, 2, 9, 4, 11, 6, 1];
  const MINOR = [5, 12, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10];
  const NAMES = ["C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
  const s = String(value).trim();
  if (!s) return value;
  let pitch, minor;
  if (/^\d/.test(s)) {
    const m = s.match(/^(\d{1,2})\s*([ABmd])$/i);
    if (!m) return value;
    const num = +m[1];
    if (num < 1 || num > 12) return value;
    const letter = m[2].toUpperCase();
    minor = letter === "A" || letter === "M";
    const table = minor ? MINOR : MAJOR;
    const camelot = letter === "A" || letter === "B" ? num : ((num + 6) % 12) + 1;
    pitch = table.indexOf(camelot);
    if (pitch < 0) return value;
  } else {
    const m = s.match(/^([A-Ga-g])([#♯sb♭]?)\s*(.*)$/);
    if (!m) return value;
    const base = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 }[m[1].toUpperCase()];
    let p = base + (/[#♯s]/.test(m[2]) ? 1 : /[b♭]/.test(m[2]) ? -1 : 0);
    pitch = ((p % 12) + 12) % 12;
    const mode = m[3].replace(/[\s-]/g, "").toLowerCase();
    if (mode === "" || mode.startsWith("maj")) minor = false;
    else if (mode === "m" || mode.startsWith("min")) minor = true;
    else return value;
  }
  if (style === "musical") return NAMES[pitch] + (minor ? "m" : "");
  const table = minor ? MINOR : MAJOR;
  if (style === "openkey") return (((table[pitch] + 4) % 12) + 1) + (minor ? "m" : "d");
  return table[pitch] + (minor ? "A" : "B"); // camelot
}

// ---- browser-only mock (no effect inside Tauri) ----
function mockInvoke(cmd, args) {
  mockInvoke.state = mockInvoke.state || {
    tracks: [
      { path: "/music/01 - the x factor - desert rain.mp3", format: "Mp3", tags: { artist: "The X Factor", title: "Desert Rain", album: "La Bush", year: "1996" } },
      { path: "/music/02 - wish mountain - radio.mp3", format: "Mp3", tags: { artist: "Wish Mountain", title: "Radio", album: "La Bush", year: "1996" } },
      { path: "/music/03 - u-hi - feel it.mp3", format: "Mp3", tags: { artist: "U-Hi?", title: "Feel It", album: "La Bush", year: "1996" } },
    ],
    history: [],
  };
  const s = mockInvoke.state;
  const findTrack = (p) => s.tracks.find((x) => x.path === p);
  switch (cmd) {
    case "open_library":
      return Promise.resolve();
    case "read_cover_image":
      // Browser-dev stand-in: echo a tiny 1x1 PNG as the "read" cover so the
      // embed flow can be exercised without touching disk (#133).
      return Promise.resolve({
        mime: "image/png",
        data_base64:
          "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M8AAAMBAQDJ/pLvAAAAAElFTkSuQmCC",
      });
    case "open_drop": {
      // Mirror the backend resolver enough to exercise both modes + grouping:
      // a path with no file extension is treated as a folder, everything else a
      // loose file. One folder alone → library; anything else → file-set.
      const paths = args.paths || [];
      const isFolder = (p) => !/\.[a-z0-9]+$/i.test(p.replace(/[\\/]+$/, ""));
      const dirs = paths.filter(isFolder).map((d) => "/dropped/" + d.replace(/[\\/]+$/, ""));
      const files = paths.filter((p) => !isFolder(p));
      const mk = (path, artist, title) => ({
        path,
        format: "Mp3",
        tags: { artist, title, album: "Dropped", year: "2020" },
      });
      if (dirs.length === 1 && files.length === 0) {
        const root = dirs[0];
        // Nested subfolders + a loose root file, to exercise the folder-path
        // group labels (#129).
        s.tracks = [
          mk(`${root}/CD1/01 a.mp3`, "Library", "A"),
          mk(`${root}/CD1/02 b.mp3`, "Library", "B"),
          mk(`${root}/CD2/01 c.mp3`, "Library", "C"),
          mk(`${root}/00 root note.mp3`, "Library", "Root file"),
        ];
        return Promise.resolve({ mode: "library", root, folders: [] });
      }
      const all = [];
      dirs.forEach((base, di) => {
        all.push(mk(`${base}/01 track.mp3`, `Folder ${di + 1}`, "Track 1"));
        all.push(mk(`${base}/02 track.mp3`, `Folder ${di + 1}`, "Track 2"));
      });
      files.forEach((f) => all.push(mk(`/dropped/${f}`, "Loose", f)));
      s.tracks = all;
      return Promise.resolve({ mode: "files", root: "/dropped", folders: dirs });
    }
    case "open_release_page":
      // No system browser in the dev mock; just echo so the click is testable.
      console.log(`[mock] open_release_page ${args.source} ${args.id}`);
      return Promise.resolve();
    case "save_release_images": {
      // Mirror the backend naming + conflict flow so the confirm dialog can be
      // exercised: names are positional, and a previously-saved name conflicts
      // until overwrite is confirmed.
      const names = args.urls.map((_, i) =>
        i === 0 ? "folder.jpg" : i === 1 ? "cover.jpg" : `cover-${i - 1}.jpg`,
      );
      s.savedImages = s.savedImages || new Set();
      const conflicts = args.overwrite ? [] : names.filter((n) => s.savedImages.has(n));
      if (conflicts.length) return Promise.resolve({ written: [], conflicts });
      names.forEach((n) => s.savedImages.add(n));
      return Promise.resolve({ written: names.map((n) => `/music/${n}`), conflicts: [] });
    }
    case "list_tracks":
      return Promise.resolve(s.tracks);
    case "preview_rename": {
      const changes = args.paths
        .map((p) => {
          const t = findTrack(p);
          if (!t) return null;
          const dir = p.slice(0, p.lastIndexOf("/") + 1);
          const ext = p.slice(p.lastIndexOf("."));
          const name = args.mask
            .replace("%artist%", t.tags.artist || "")
            .replace("%title%", t.tags.title || "")
            .replace("%album%", t.tags.album || "")
            .replace("%year%", t.tags.year || "");
          const rename_to = dir + name + ext;
          return rename_to === p ? null : { path: p, rename_to, tag_changes: [] };
        })
        .filter(Boolean);
      return Promise.resolve({ description: "Rename by mask", changes });
    }
    case "probe_tags_from_name": {
      const subject = mockNameSubject(args.path, args.mask);
      const fields = mockExtractFromName(args.mask, subject, args.cleanup);
      return Promise.resolve({ subject, fields: fields || [], matched: !!fields });
    }
    case "preview_tags_from_name": {
      const changes = args.paths
        .map((p) => {
          const t = findTrack(p);
          if (!t) return null;
          const fields = mockExtractFromName(args.mask, mockNameSubject(p, args.mask), args.cleanup);
          if (!fields) return null;
          const tag_changes = fields
            .filter(([field, value]) => value && (t.tags[field] || "") !== value)
            .map(([field, value]) => ({
              field,
              old: t.tags[field] || null,
              new: value,
              invalid: false,
            }));
          return tag_changes.length ? { path: p, rename_to: null, tag_changes } : null;
        })
        .filter(Boolean);
      return Promise.resolve({ description: "Tags from name", changes });
    }
    case "preview_transform": {
      // Mirrors the backend closely enough to exercise the dialog: literal
      // replace plus title-casing, over tags or the file name.
      const applyRules = (value) => mockApplyRules(value, args.rules);
      const changes = args.paths
        .map((p) => {
          const t = findTrack(p);
          if (!t) return null;
          if (args.scope === "filename") {
            const dir = p.slice(0, p.lastIndexOf("/") + 1);
            const base = p.slice(p.lastIndexOf("/") + 1, p.lastIndexOf("."));
            const ext = p.slice(p.lastIndexOf("."));
            const renamed = applyRules(base);
            return renamed === base ? null : { path: p, rename_to: `${dir}${renamed}${ext}`, tag_changes: [] };
          }
          if (args.scope === "fileext") {
            const stem = p.slice(0, p.lastIndexOf("."));
            const ext = p.slice(p.lastIndexOf(".") + 1);
            const renamed = applyRules(ext);
            if (renamed === ext || !renamed.trim() || /[/\\.]/.test(renamed)) return null;
            return { path: p, rename_to: `${stem}.${renamed}`, tag_changes: [] };
          }
          const tag_changes = [];
          for (const [field, value] of Object.entries(t.tags)) {
            if (args.scope !== "tags" && args.scope !== field) continue;
            const next = applyRules(value);
            if (next !== value) tag_changes.push({ field, old: value, new: next });
          }
          return tag_changes.length ? { path: p, rename_to: null, tag_changes } : null;
        })
        .filter(Boolean);
      return Promise.resolve({ description: "Transform", changes });
    }
    // The checklist run (#137): groups in order, each seeing the last one's
    // result, ending in one change per file — same shape as the backend.
    case "preview_transform_groups": {
      const changes = args.paths
        .map((p) => {
          const t = findTrack(p);
          if (!t) return null;
          const dir = p.slice(0, p.lastIndexOf("/") + 1);
          let name = p.slice(p.lastIndexOf("/") + 1, p.lastIndexOf("."));
          let ext = p.slice(p.lastIndexOf(".") + 1);
          const tags = { ...t.tags };
          for (const group of args.groups) {
            if (group.scope === "filename") {
              const next = mockApplyRules(name, group.rules);
              if (next.trim()) name = next;
            } else if (group.scope === "fileext") {
              const next = mockApplyRules(ext, group.rules);
              if (next.trim() && !/[/\\.]/.test(next)) ext = next;
            } else {
              for (const [field, value] of Object.entries(tags)) {
                if (group.scope !== "tags" && group.scope !== field) continue;
                tags[field] = mockApplyRules(value, group.rules);
              }
            }
          }
          const tag_changes = Object.entries(tags)
            .filter(([field, value]) => value !== t.tags[field])
            .map(([field, value]) => ({ field, old: t.tags[field], new: value }));
          const file_name = `${name}.${ext}`;
          const renamed = file_name !== p.slice(p.lastIndexOf("/") + 1);
          if (!tag_changes.length && !renamed) return null;
          return { path: p, rename_to: renamed ? `${dir}${file_name}` : null, tag_changes };
        })
        .filter(Boolean);
      return Promise.resolve({ description: "Transform", changes });
    }
    case "preview_move": {
      const changes = args.paths
        .map((p) => {
          const t = findTrack(p);
          if (!t) return null;
          const ext = p.slice(p.lastIndexOf("."));
          const rendered = args.mask
            .replace("%albumartist%", t.tags.albumartist || t.tags.artist || "")
            .replace("%artist%", t.tags.artist || "")
            .replace("%title%", t.tags.title || "")
            .replace("%album%", t.tags.album || "")
            .replace("%year%", t.tags.year || "")
            .replace("%track%", t.tags.track || "")
            .replace("%genre%", t.tags.genre || "");
          if (rendered.split("/").some((part) => !part.trim() || part === "..")) return null;
          return { path: p, rename_to: `/music/${rendered}${ext}`, tag_changes: [] };
        })
        .filter(Boolean);
      return Promise.resolve({ description: "Reorganize by mask", changes });
    }
    case "preview_tag_edits": {
      const byPath = {};
      for (const e of args.edits) {
        const t = findTrack(e.path);
        if (!t) continue;
        const old = t.tags[e.field] || null;
        const nv = e.value || null;
        if (old === nv) continue;
        (byPath[e.path] = byPath[e.path] || []).push({ field: e.field, old, new: nv });
      }
      const changes = Object.entries(byPath).map(([path, tag_changes]) => ({ path, rename_to: null, tag_changes }));
      return Promise.resolve({ description: "Edit tags", changes });
    }
    case "preview_clear_tags": {
      const changes = [];
      for (const p of args.paths) {
        const t = findTrack(p);
        if (!t) continue;
        const tag_changes = Object.entries(t.tags)
          .filter(([, v]) => v)
          .map(([field, old]) => ({ field, old, new: null }));
        if (tag_changes.length) changes.push({ path: p, rename_to: null, tag_changes, cover_change: null });
      }
      return Promise.resolve({ description: "Clear tags", changes });
    }
    case "apply_plan":
      for (const c of args.plan.changes) {
        const t = findTrack(c.path);
        if (!t) continue;
        if (c.rename_to) t.path = c.rename_to;
        for (const tc of c.tag_changes) t.tags[tc.field] = tc.new || "";
      }
      s.history.unshift({ id: s.history.length + 1, description: args.plan.description, applied_at: 0 });
      return Promise.resolve({ id: s.history.length, description: args.plan.description, applied_at: 0 });
    case "history":
      return Promise.resolve(s.history);
    case "undo":
      s.history.shift();
      return Promise.resolve();
    case "preview_cover_embed": {
      const changes = args.paths.map((p) => ({
        path: p,
        rename_to: null,
        tag_changes: [],
        cover_change: { old: null, new: args.cover },
      }));
      return Promise.resolve({ description: "Embed cover art", changes });
    }
    case "read_cover_summary": {
      // Mock: pretend the mock tracks carry a cover if their tags say so.
      const svg = (fill) => ({
        mime: "image/svg+xml",
        data_base64: btoa(`<svg xmlns='http://www.w3.org/2000/svg' width='40' height='40'><rect width='40' height='40' fill='${fill}'/></svg>`),
      });
      const covers = args.paths.map((p) => {
        const t = findTrack(p);
        return t && t.cover ? svg(t.cover) : null;
      });
      const total = covers.length;
      const with_cover = covers.filter(Boolean).length;
      const uniq = [];
      for (const c of covers) if (!uniq.some((u) => JSON.stringify(u) === JSON.stringify(c))) uniq.push(c);
      const distinct = uniq.length > 1;
      const samples = [];
      for (const c of covers) {
        if (c && !samples.some((s) => JSON.stringify(s) === JSON.stringify(c))) {
          samples.push(c);
          if (samples.length === 3) break;
        }
      }
      return Promise.resolve({ total, with_cover, distinct, samples });
    }
    case "preview_cover_remove": {
      const changes = args.paths
        .map((p) => {
          const t = findTrack(p);
          if (!t || !t.cover) return null;
          return { path: p, rename_to: null, tag_changes: [], cover_change: { old: { mime: "image/svg+xml", data_base64: "" }, new: null } };
        })
        .filter(Boolean);
      return Promise.resolve({ description: "Remove cover art", changes });
    }
    case "export_cover": {
      // Pretend odd-indexed files have no cover so the skip path is exercised;
      // dedupe same-folder targets like the real backend does.
      const written = [];
      const seen = new Set();
      const skipped_no_cover = [];
      args.paths.forEach((p, i) => {
        if (i % 2 !== 0) {
          skipped_no_cover.push(p);
          return;
        }
        const dir = p.slice(0, p.lastIndexOf("/") + 1);
        const target = `${dir}${args.basename}.jpg`;
        if (!seen.has(target)) {
          seen.add(target);
          written.push(target);
        }
      });
      return Promise.resolve({ written, skipped_no_cover });
    }
    case "read_external_cover":
      // Browser mock: no sibling cover unless a test injects one.
      return Promise.resolve(mockInvoke.state?.externalCover ?? null);
    case "find_duplicates":
      // Mock: pretend the first track has a copy in a /dupes subfolder.
      return Promise.resolve(
        (mockInvoke.state?.tracks || []).slice(0, 1).map((t) => ({
          key: `${t.tags.artist} — ${t.tags.title}`,
          files: [
            { path: t.path, artist: t.tags.artist, title: t.tags.title, album: t.tags.album || "", duration_secs: 278, size_bytes: 8123456, bitrate_kbps: 320 },
            { path: `/music/dupes/${fileName(t.path)}`, artist: t.tags.artist, title: t.tags.title, album: t.tags.album || "", duration_secs: 278, size_bytes: 5242880, bitrate_kbps: 192 },
          ],
        })),
      );
    case "auto_align": {
      // Mock: an equal ISRC is an exact match (#54); otherwise fall back to an
      // exact title match, mirroring the backend. Returns { track, by_isrc }.
      const norm = (s) => (s || "").replace(/[^a-z0-9]/gi, "").toUpperCase();
      const titles = args.tracks.map((t) => t.title.toLowerCase());
      const isrcs = args.tracks.map((t) => norm(t.isrc));
      return Promise.resolve(
        args.paths.map((p) => {
          const t = findTrack(p);
          if (!t) return null;
          const localIsrc = norm(t.tags["isrc"]);
          const byIsrc = localIsrc ? isrcs.findIndex((c) => c && c === localIsrc) : -1;
          if (byIsrc >= 0) return { track: byIsrc, by_isrc: true };
          const i = titles.indexOf((t.tags.title || "").toLowerCase());
          return i >= 0 ? { track: i, by_isrc: false } : null;
        })
      );
    }
    case "export_playlist":
    case "export_csv":
    case "export_html":
    case "export_xml":
    case "export_report":
      // The real backend writes into the library root and returns the path.
      return Promise.resolve(`/music/${args.fileName}`);
    case "player_play":
      mockPlayer.current = args.path;
      mockPlayer.next = null;
      mockPlayer.restart(0);
      return Promise.resolve();
    case "player_set_next":
      if (mockPlayer.current && !mockPlayer.next) mockPlayer.next = args.path;
      return Promise.resolve();
    case "player_pause":
      if (mockPlayer.current && !mockPlayer.pausedAt) mockPlayer.pausedAt = Date.now();
      return Promise.resolve();
    case "player_resume":
      if (mockPlayer.pausedAt) {
        mockPlayer.restart(mockPlayer.position());
      }
      return Promise.resolve();
    case "player_stop":
      mockPlayer.current = null;
      mockPlayer.next = null;
      mockPlayer.pausedAt = 0;
      return Promise.resolve();
    case "player_seek":
      if (mockPlayer.current) mockPlayer.restart(args.secs);
      return Promise.resolve();
    case "player_set_volume":
      // Browser dev has no audio thread; just accept it so the UI path is live.
      mockPlayer.volume = args.level;
      return Promise.resolve();
    case "player_status":
      return Promise.resolve(mockPlayer.status());
    // The placeholder reference (#148). A trimmed stand-in: enough of each group
    // for the popover's grouping, filtering and insertion to be exercised in the
    // browser. The real list comes off the parser's own tables.
    case "mask_placeholders":
      return Promise.resolve(
        [
          ["artist", "Track artist", "Tags", true, true],
          ["title", "Track title", "Tags", true, true],
          ["track", "Track number (pads to two digits)", "Tags", true, true],
          ["catalognumber", "Label catalogue number", "Tags", true, true],
          ["filename", "File name without the extension", "File", true, false],
          ["foldername", "Containing folder", "File", true, false],
          ["_bitrate", "Bitrate, kbps", "Technical", true, false],
          ["_length", "Duration, m:ss", "Technical", true, false],
          ["side", "Vinyl side letter, from the disc number", "Special", true, false],
          ["skip", "Matches and discards a run of text", "Special", false, true],
        ].map(([name, description, group, render, extract]) => ({
          token: `%${name}%`,
          name,
          description,
          group,
          render,
          extract,
        }))
      );
    // The import-field catalogue (#152). A trimmed stand-in — enough rows,
    // including the two-key release-id one, to drive the settings section.
    // Mask-defined columns (#150). The dev mock renders a couple of plain
    // placeholders so the column can be seen filling in; the real grammar is
    // mask.rs and this is not a second implementation of it.
    case "render_column": {
      // The real backend parses the mask first, so an unknown placeholder is an
      // error before any file is touched — the editor relies on that to reject a
      // typo once instead of blanking the column on every repaint. Model it, or
      // the dev path silently accepts patterns the app would refuse.
      const known = [
        "artist", "title", "album", "albumartist", "track", "tracktotal", "disc",
        "disctotal", "year", "genre", "comment", "composer", "publisher", "bpm",
        "isrc", "key", "catalognumber", "url", "media", "side", "skip",
        "filename", "fileext", "filenameext", "filepath", "foldername",
        "foldername2", "foldername3", "_length", "_length_sec", "_bitrate",
        "_samplerate", "_channels", "_codec", "_filesize", "_filesize_bytes",
        "_filedate",
      ];
      for (const [, name] of String(args.pattern || "").matchAll(/%([a-z_0-9]+?)(?::\d+)?%/gi)) {
        if (!known.includes(name.toLowerCase())) {
          return Promise.reject(`unknown placeholder: ${name}`);
        }
      }
      return Promise.resolve(
        (args.paths || []).map((path) => {
          const t = (s.tracks || []).find((x) => x.path === path);
          return String(args.pattern || "").replace(/%([a-z_]+)%/gi, (_, name) => {
            const key = name.toLowerCase();
            if (key === "filename") return fileName(path).replace(/\.[^.]*$/, "");
            if (key === "_codec") return "MP3";
            return (t && t.tags && t.tags[key]) || "";
          });
        })
      );
    }
    case "import_fields":
      return Promise.resolve([
        { keys: ["title"], label: "Title" },
        { keys: ["artist"], label: "Artist" },
        { keys: ["album"], label: "Album" },
        { keys: ["genre"], label: "Genre" },
        { keys: ["url"], label: "Release webpage" },
        { keys: ["custom:RELEASECOUNTRY"], label: "Release country" },
        { keys: ["custom:DISCOGS_RELEASE_ID", "custom:MUSICBRAINZ_ALBUMID"], label: "Release id" },
      ]);
    case "saved_discogs_token":
      return Promise.resolve(mockInvoke.state?.token || "");
    case "save_discogs_token":
      mockInvoke.state = mockInvoke.state || {};
      mockInvoke.state.token = args.token;
      return Promise.resolve();
    case "load_settings":
      return Promise.resolve(
        mockInvoke.state?.settings || {
          proxy: "",
          rate_limit_per_min: 0,
          id3_v23: false,
          read_priority: [],
          cover_max_px: 0,
          cover_quality: 85,
          import_skip_fields: [],
          multi_value_separator: "",
        }
      );
    case "save_settings":
      mockInvoke.state = mockInvoke.state || {};
      mockInvoke.state.settings = args.settings;
      return Promise.resolve();
    case "provider_search":
      // MusicBrainz hits carry no cover in search and use MBID string ids (#33).
      if (args.source === "musicbrainz") {
        return Promise.resolve([
          { id: "aeb1c1c0-0000-0000-0000-000000000001", artist: "Various Artists", title: "La Bush", year: 1996, score: 1.0, thumb_url: null, cover_url: null, country: "BE", label: "Antler-Subway", format: "CD", catalog_number: "TOTH 006" },
        ]);
      }
      {
        // Fake a paginated Discogs response so "Load more" / Stop (#95/#96) can
        // be exercised in the browser mock: 23 hits total, sliced by page.
        const TOTAL = 23;
        const per = args.query?.per_page || 10;
        const page = args.query?.page || 1;
        const start = (page - 1) * per;
        const hits = [];
        for (let i = start; i < Math.min(start + per, TOTAL); i++) {
          const n = i + 1;
          hits.push({
            id: String(300000 + n),
            artist: "Various",
            title: `La Bush Vol. ${n}`,
            year: 1996,
            score: 1 - i * 0.01,
            thumb_url: "https://img/1t.jpg",
            cover_url: "https://img/1c.jpg",
            country: "Belgium",
            label: "Antler-Subway",
            format: n % 2 ? "Vinyl, LP" : "CD, Mixed",
            catalog_number: `TOTH ${String(n).padStart(3, "0")}`,
          });
        }
        // Media-type filter (#103): mirror the provider's `format` narrowing.
        const fmt = args.query?.format;
        return Promise.resolve(
          fmt ? hits.filter((h) => h.format.toLowerCase().includes(fmt.toLowerCase())) : hits
        );
      }
    case "provider_fetch_release":
      if (args.source === "musicbrainz") {
        return Promise.resolve({
          id: args.releaseId,
          artist: "Various Artists",
          title: "La Bush",
          year: 1996,
          genres: ["house", "techno"], // MusicBrainz genre tags; no styles
          styles: [],
          tracks: [
            { position: "1", artist: "The X Factor", title: "Desert Rain", duration_secs: 278 },
            { position: "2", artist: "Wish Mountain", title: "Radio", duration_secs: 142 },
          ],
          labels: [{ name: "Antler-Subway", catalog_number: "AS 5606" }],
          cover_image_url: `https://coverartarchive.org/release/${args.releaseId}/front`,
          // CAA reports no dimensions, so resolution won't show for MusicBrainz.
          images: [
            { url: `https://coverartarchive.org/release/${args.releaseId}/front`, width: 0, height: 0 },
          ],
        });
      }
      return Promise.resolve({
        id: args.releaseId,
        artist: "Various",
        title: "La Bush - Music From The Temple Of House",
        year: 1996,
        genres: ["Electronic"],
        styles: ["Trance", "Tribal", "Techno"],
        // A two-disc set, so the browser path exercises the disc pass-through
        // (#146): Discogs states the disc in the position, and the count in the
        // format quantity.
        tracks: [
          { position: "1-1", disc: 1, artist: "The X Factor", title: "Desert Rain", duration_secs: 278 },
          { position: "1-2", disc: 1, artist: "Wish Mountain", title: "Radio", duration_secs: 142 },
          { position: "2-1", disc: 2, artist: "West Coast Connection", title: "Voodoo Rhythm", duration_secs: 321 },
        ],
        disc_total: 2,
        // Two catalogue numbers from the same label (#90) — exercises the picker.
        labels: [
          { name: "Antler-Subway", catalog_number: "AS 5606" },
          { name: "Antler-Subway", catalog_number: "7243 8 52174 2 5" },
        ],
        cover_image_url: "https://img.discogs.com/mock/front.jpg",
        // Discogs states per-image dimensions (#102): primary + two secondaries.
        images: [
          { url: "https://img.discogs.com/mock/front.jpg", width: 600, height: 594 },
          { url: "https://img.discogs.com/mock/back.jpg", width: 600, height: 590 },
          { url: "https://img.discogs.com/mock/cd.jpg", width: 500, height: 500 },
        ],
      });
    case "provider_fetch_image":
      // A tiny solid-color PNG so the release-view cover has something to show.
      return Promise.resolve({
        mime: "image/png",
        data_base64:
          "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
      });
    case "preview_import": {
      const sideCounters = {}; // per-disc running track number for bare vinyl sides
      const changes = args.paths.map((p, i) => {
        const t = findTrack(p);
        const rt = args.selection.tracks[i];
        const tag_changes = [
          { field: "album", old: t ? t.tags.album || null : null, new: args.selection.album },
        ];
        if (args.selection.genre) {
          tag_changes.push({ field: "genre", old: t ? t.tags.genre || null : null, new: args.selection.genre });
        }
        if (args.selection.release_id) {
          const key = args.selection.source === "musicbrainz" ? "custom:MUSICBRAINZ_ALBUMID" : "custom:DISCOGS_RELEASE_ID";
          tag_changes.push({ field: key, old: t ? t.tags[key] || null : null, new: args.selection.release_id });
        }
        if (args.selection.label) {
          tag_changes.push({ field: "publisher", old: t ? t.tags.publisher || null : null, new: args.selection.label });
        }
        if (args.selection.catalog_number) {
          tag_changes.push({ field: "catalognumber", old: t ? t.tags.catalognumber || null : null, new: args.selection.catalog_number });
        }
        if (args.selection.media_type) {
          tag_changes.push({ field: "media", old: t ? t.tags.media || null : null, new: args.selection.media_type });
        }
        if (rt) {
          tag_changes.push({ field: "title", old: t ? t.tags.title || null : null, new: rt.title });
          tag_changes.push({ field: "artist", old: t ? t.tags.artist || null : null, new: rt.artist });
          // Mirror the backend: when the vinyl toggle is on and the position is a
          // side, map the side to a disc (overwriting a default disc) and restart
          // the track number per side; otherwise the plain number / row index.
          const parsed = args.vinylSidesToDisc ? parseVinylPosition(rt.position) : null;
          let num;
          if (parsed) {
            num = parsed.track ?? String((sideCounters[parsed.disc] = (sideCounters[parsed.disc] || 0) + 1));
            const curDisc = t ? t.tags.disc || null : null;
            if (curDisc !== parsed.disc) {
              tag_changes.push({ field: "disc", old: curDisc, new: parsed.disc });
            }
          } else {
            const digits = String(rt.position || "").match(/\d+$/);
            num = digits ? String(parseInt(digits[0], 10)) : String(i + 1);
          }
          const curTrack = t ? t.tags.track || null : null;
          if (curTrack !== num) {
            tag_changes.push({ field: "track", old: curTrack, new: num });
          }
        }
        return { path: p, rename_to: null, tag_changes };
      });
      return Promise.resolve({ description: "Import Discogs release", changes });
    }
    default:
      return Promise.reject(`unknown command: ${cmd}`);
  }
}

export { mockInvoke };
