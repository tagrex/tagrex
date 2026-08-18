// The settings slide-over (#79, #143 split it out of app.js).
//
// App-wide preferences, opened from the top-bar gear: the Discogs token,
// network options, tag write/read defaults, sidecar extensions, and the display
// section that drives the preference module. It reads the whole SettingsDto and
// writes the whole thing back — `save_settings` overwrites settings.json with
// what it is given, so the saved snapshot is spread rather than replaced.
import { el, ico, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { enablePointerReorder } from "./reorder.js";
import { actionGroups, savedSettings, setSavedSettings } from "./state.js";
import {
  applyBadgeFont,
  clampTableFont,
  clampTracklistFont,
  applyCheckboxCol,
  applyTableFont,
  applyTheme,
  applyTracklistFont,
  applyValueFont,
  badgeFont,
  checkboxColEnabled,
  tableFontPx,
  themeMode,
  tracklistFontPx,
  valueFont,
} from "./prefs.js";

// ---- settings slide-over (#79) ----
// App-wide preferences, opened from the top-bar gear. The Discogs token lives
// here now (moved out of TAGGER › ONLINE); the search still reads it via the
// same #discogs-token input.
let id3Choice = "v24"; // "v23" | "v24", mirrored by the segmented control
// Whether an import brings the release's cover (#207): "never" | "if-missing"
// | "always", mirrored by its segmented control.
let importCoverChoice = "if-missing";

function updateSettingsDot() {
  el("settings-open").classList.toggle("has-token", !!el("discogs-token").value.trim());
}

function setId3Choice(choice) {
  id3Choice = choice;
  el("set-id3")
    .querySelectorAll(".seg-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.id3 === choice));
}

function setImportCoverChoice(choice) {
  importCoverChoice = choice;
  el("set-import-cover")
    .querySelectorAll(".seg-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.importCover === choice));
}

// Reflect + apply a theme choice from the segmented control (live, like the font
// slider — persisted immediately so the preview sticks).
function setThemeChoice(mode) {
  applyTheme(mode);
  el("set-theme")
    .querySelectorAll(".seg-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.themeMode === mode));
}

function setBadgeFontChoice(mode) {
  applyBadgeFont(mode);
  el("set-badge-font")
    .querySelectorAll(".seg-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.badgeFont === mode));
}

// Same live treatment for the value-font choice — the swap is visible behind the
// settings sheet, so applying on click beats waiting for Save.
function setValueFontChoice(mode) {
  applyValueFont(mode);
  el("set-value-font")
    .querySelectorAll(".seg-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.valueFont === mode));
}

// Tag-read priority (#84): the order tag blocks are consulted when a file
// carries more than one. Persisted as an ordered list of keys; the default
// order matches the common case (ID3v2 first).
const PRIO_KEYS = ["id3v2", "vorbis", "ape"];
const PRIO_LABELS = { id3v2: "ID3v2", vorbis: "Vorbis Comments", ape: "APE" };
let readPriority = PRIO_KEYS.slice();

// Normalize a saved/loaded list to exactly the known keys in the given order,
// appending any known key the list omitted so all three always show.
function normalizePriority(list) {
  const known = new Set(PRIO_KEYS);
  const seen = [];
  for (const k of Array.isArray(list) ? list : []) {
    if (known.has(k) && !seen.includes(k)) seen.push(k);
  }
  for (const k of PRIO_KEYS) if (!seen.includes(k)) seen.push(k);
  return seen;
}

function renderPrioList() {
  const list = el("set-prio");
  list.innerHTML = "";
  for (const key of readPriority) list.appendChild(prioItem(key));
}

// Reset read priority to the default order (#91). Takes effect on Save, like the
// rest of the settings panel.
function resetPriority() {
  readPriority = PRIO_KEYS.slice();
  renderPrioList();
}

function prioItem(key) {
  const li = document.createElement("li");
  li.className = "prio-item";
  li.dataset.key = key;

  const grip = document.createElement("span");
  grip.className = "prio-grip";
  grip.innerHTML = ico("grip");
  grip.title = "Drag to reorder";
  enablePointerReorder(grip, li, el("set-prio"), ".prio-item", (dragged, target, below) => {
    const order = readPriority.filter((k) => k !== dragged);
    let to = order.indexOf(target);
    if (below) to += 1;
    order.splice(to, 0, dragged);
    readPriority = order;
    renderPrioList();
  });

  const label = document.createElement("span");
  label.className = "prio-label";
  label.textContent = PRIO_LABELS[key] || key;

  li.append(grip, label);
  return li;
}

// Default sidecar extensions (#58), mirroring the backend's default set. Shown
// when settings have never been saved.
const DEFAULT_SIDECAR_EXTS = ["lrc", "cue", "txt", "jpg", "jpeg", "png"];

// Parse the sidecar-extensions input: split on spaces/commas, drop a leading
// dot, lower-case, de-duplicate, and drop empties.
function parseSidecarExts(raw) {
  return [
    ...new Set(
      (raw || "")
        .split(/[\s,]+/)
        .map((e) => e.trim().replace(/^\./, "").toLowerCase())
        .filter(Boolean)
    ),
  ];
}

// ---- which fields an online import may write (#152) ----
//
// The rows come from the backend (`import_fields`), which is also what
// `preview_import` is tested against, so this list cannot offer a field the
// import doesn't produce or miss one it does.
//
// Stored as a DENY list of storage keys: a ticked box means "not in the list".
// That way an absent or older settings.json means "write everything", which is
// the historical behaviour, and a field added later is written by default rather
// than silently excluded.
let importFieldRows = [];

async function renderImportFields(skip) {
  const host = el("set-import-fields");
  if (!importFieldRows.length) {
    try {
      importFieldRows = await invoke("import_fields", {});
    } catch (e) {
      host.innerHTML = "";
      return; // no catalogue — leave the section empty rather than guessing
    }
  }
  const denied = new Set(skip || []);
  host.innerHTML = "";
  for (const field of importFieldRows) {
    const row = document.createElement("label");
    row.className = "import-field";
    const box = document.createElement("input");
    box.type = "checkbox";
    // A row is on unless every key it governs is denied — the release-id row
    // covers both provider keys.
    box.checked = !field.keys.every((k) => denied.has(k));
    box.dataset.keys = field.keys.join(" ");
    const name = document.createElement("span");
    name.textContent = field.label;
    row.append(box, name);
    host.appendChild(row);
  }
}

// The deny list the form currently describes.
function importSkipFromForm() {
  const skip = [];
  el("set-import-fields")
    .querySelectorAll("input[type=checkbox]")
    .forEach((box) => {
      if (!box.checked) skip.push(...box.dataset.keys.split(" "));
    });
  return skip;
}

function setAllImportFields(on) {
  el("set-import-fields")
    .querySelectorAll("input[type=checkbox]")
    .forEach((box) => (box.checked = on));
}

// ---- Beatport sign-in (#162) ----
// The account is not part of SettingsDto: the backend owns the tokens and this
// only reflects them, so signing in and out take effect at once rather than on
// Save. Sign-in opens Beatport's own login window — nothing is typed here.
async function refreshBeatportAccount() {
  let status = { authorized: false, username: "" };
  try {
    status = await invoke("beatport_status", {});
  } catch (e) {
    /* treat an unreadable session as signed out */
  }
  el("beatport-account").textContent = status.authorized
    ? `Signed in${status.username ? ` as ${status.username}` : ""}`
    : "Not signed in";
  el("beatport-signin").textContent = status.authorized ? "Sign in again…" : "Sign in…";
  el("beatport-signout").hidden = !status.authorized;
}

async function beatportSignIn() {
  const btn = el("beatport-signin");
  btn.disabled = true;
  el("beatport-account").textContent = "Waiting for the Beatport window…";
  try {
    const username = await invoke("beatport_login", {});
    toast(username ? `Signed in to Beatport as ${username}` : "Signed in to Beatport");
  } catch (e) {
    toast(String(e), true);
  } finally {
    btn.disabled = false;
    await refreshBeatportAccount();
  }
}

async function beatportSignOut() {
  try {
    await invoke("beatport_logout", {});
    toast("Signed out of Beatport");
  } catch (e) {
    toast(String(e), true);
  }
  await refreshBeatportAccount();
}

async function openSettings() {
  await refreshBeatportAccount();
  // Populate from saved values (the token is already in #discogs-token).
  try {
    const s = await invoke("load_settings", {});
    el("set-proxy").value = s.proxy || "";
    el("set-rate").value = s.rate_limit_per_min || 0;
    setId3Choice(s.id3_v23 ? "v23" : "v24");
    setImportCoverChoice(s.import_cover || "if-missing");
    el("set-cover-max").value = s.cover_max_px || 0;
    el("set-cover-quality").value = s.cover_quality || 85;
    readPriority = normalizePriority(s.read_priority);
    // The separator is whitespace-significant ("; " is not ";"), so it goes
    // into the box verbatim and comes back verbatim — no trim anywhere (#46).
    el("set-multi-sep").value = s.multi_value_separator || "";
    el("set-carry-sidecars").checked = s.carry_sidecars !== false;
    el("set-carry-extras").checked = s.carry_folder_extras !== false;
    el("set-sidecar-exts").value = (s.sidecar_extensions && s.sidecar_extensions.length
      ? s.sidecar_extensions
      : DEFAULT_SIDECAR_EXTS
    ).join(" ");
    await renderImportFields(s.import_skip_fields);
  } catch (e) {
    /* defaults already in the DOM */
    readPriority = PRIO_KEYS.slice();
    el("set-multi-sep").value = "";
    el("set-carry-sidecars").checked = true;
    el("set-carry-extras").checked = true;
    el("set-sidecar-exts").value = DEFAULT_SIDECAR_EXTS.join(" ");
    await renderImportFields([]);
  }
  // Display prefs live in localStorage, not the backend settings.
  setThemeChoice(themeMode());
  el("set-checkbox-col").checked = checkboxColEnabled();
  setValueFontChoice(valueFont());
  el("set-table-font").value = tableFontPx();
  el("set-table-font-val").textContent = `${tableFontPx()}px`;
  el("set-tracklist-font").value = tracklistFontPx();
  el("set-tracklist-font-val").textContent = `${tracklistFontPx()}px`;
  setBadgeFontChoice(badgeFont());
  renderPrioList();
  el("settings").hidden = false;
}

function closeSettings() {
  el("settings").hidden = true;
}

async function saveSettings() {
  // Everything from here on is inside the try, not just the two invokes: the
  // object below is built from the form and from imported state, and when a
  // `savedSettings` that was never imported threw there, the failure escaped as
  // an unhandled rejection — no toast, sheet left open, nothing saved, and no
  // sign anything had gone wrong (#156).
  try {
    const token = el("discogs-token").value.trim();
    // Spread the last-known settings so we keep fields this form doesn't edit —
    // notably the saved action groups (#57), which also live in settings.json.
    const settings = {
      ...savedSettings,
      proxy: el("set-proxy").value.trim(),
      rate_limit_per_min: Math.max(0, parseInt(el("set-rate").value, 10) || 0),
      id3_v23: id3Choice === "v23",
      read_priority: readPriority.slice(),
      multi_value_separator: el("set-multi-sep").value,
      cover_max_px: Math.max(0, parseInt(el("set-cover-max").value, 10) || 0),
      cover_quality: Math.min(100, Math.max(1, parseInt(el("set-cover-quality").value, 10) || 85)),
      action_groups: actionGroups,
      carry_sidecars: el("set-carry-sidecars").checked,
      carry_folder_extras: el("set-carry-extras").checked,
      sidecar_extensions: parseSidecarExts(el("set-sidecar-exts").value),
      import_skip_fields: importSkipFromForm(),
      import_cover: importCoverChoice,
    };
    setSavedSettings(settings);
    // Display prefs are local-only; apply + persist before the backend
    // round-trip. (Table font size already applies live on input; persisted
    // here too.)
    applyCheckboxCol(el("set-checkbox-col").checked);
    // (Value font, like the theme, is a live control — already applied on click.)
    applyTableFont(parseInt(el("set-table-font").value, 10));
    await invoke("save_discogs_token", { token });
    await invoke("save_settings", { settings });
    updateSettingsDot();
    closeSettings();
    toast("Settings saved");
  } catch (e) {
    toast(String(e), true);
  }
}

// Discard unsaved edits: the token input is shared with the ONLINE search, so
// restore it to the saved value before closing.
async function cancelSettings() {
  try {
    el("discogs-token").value = (await invoke("saved_discogs_token", {})) || "";
  } catch (e) {
    /* leave as-is */
  }
  updateSettingsDot();
  closeSettings();
}

// ---- wire up ----
el("set-import-all").addEventListener("click", () => setAllImportFields(true));
el("set-import-none").addEventListener("click", () => setAllImportFields(false));
el("settings-open").addEventListener("click", openSettings);
el("settings-close").addEventListener("click", cancelSettings);
el("settings-cancel").addEventListener("click", cancelSettings);
el("settings-scrim").addEventListener("click", cancelSettings);
el("settings-save").addEventListener("click", saveSettings);
el("set-id3").addEventListener("click", (e) => {
  const btn = e.target.closest("[data-id3]");
  if (btn) setId3Choice(btn.dataset.id3);
});
el("set-import-cover").addEventListener("click", (e) => {
  const btn = e.target.closest("[data-import-cover]");
  if (btn) setImportCoverChoice(btn.dataset.importCover);
});
// Theme is a live control — switch immediately on click.
el("set-theme").addEventListener("click", (e) => {
  const btn = e.target.closest("[data-theme-mode]");
  if (btn) setThemeChoice(btn.dataset.themeMode);
});
// Value font is live too — swap on click so the effect shows behind the sheet.
el("set-value-font").addEventListener("click", (e) => {
  const btn = e.target.closest("[data-value-font]");
  if (btn) setValueFontChoice(btn.dataset.valueFont);
});
el("set-prio-reset").addEventListener("click", resetPriority);
el("beatport-signin").addEventListener("click", beatportSignIn);
el("beatport-signout").addEventListener("click", beatportSignOut);
// Table font size is a live control: drag to apply (and persist) immediately so
// the effect is visible behind the settings sheet.
// LAB sliders/segments are live controls too — the effect shows behind the sheet.
el("set-tracklist-font").addEventListener("input", (e) => {
  const px = clampTracklistFont(parseInt(e.target.value, 10));
  applyTracklistFont(px);
  el("set-tracklist-font-val").textContent = `${px}px`;
});
el("set-badge-font").addEventListener("click", (e) => {
  const btn = e.target.closest("[data-badge-font]");
  if (btn) setBadgeFontChoice(btn.dataset.badgeFont);
});
el("set-table-font").addEventListener("input", (e) => {
  const px = clampTableFont(parseInt(e.target.value, 10));
  applyTableFont(px);
  el("set-table-font-val").textContent = `${px}px`;
});

export { openSettings, cancelSettings, updateSettingsDot };
