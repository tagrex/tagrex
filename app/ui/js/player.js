// The preview player (#30, #143 split it out of app.js).
//
// Playback is native (rodio backend): this module sends commands and polls the
// backend's status, and everything about the transport — the queued next track,
// repeat, volume, the seek drag — is its own business. The rest of the UI only
// asks it to start a track or show the bar.
import { el, fileName, ico, placeFloating, toast } from "./dom.js";
import { t } from "./i18n.js";
import { hooks } from "./hooks.js";
import { invoke } from "./invoke.js";
import { tracks, selection, activeRowPath } from "./state.js";

const tracksBody = el("tracks-body");
const playerBar = el("player");
const plToggle = el("pl-toggle");
const plStop = el("pl-stop");
const plTitle = el("pl-title");
const plSeek = el("pl-seek");
const plTime = el("pl-time");
const plWave = el("pl-wave");
const plCover = el("pl-cover");
const plCoverImg = el("pl-cover-img");

// Playback runs in the native (rodio) backend; the UI mirrors its polled
// status. `playingPath` is the track the backend reports as current, `plPaused`
// its pause state, `plDuration` the current track's length (for the seek math).
let playingPath = null;
let plPaused = false;
let plDuration = 0;
// True while the user is dragging the seek slider, so status polls don't fight
// the drag.
let plSeeking = false;
// The track a refused seek was already reported for (#190), so the message is
// shown once rather than on every poll.
let seekRefusedFor = null;
// Poll timer handle (one interval once a library is open).
let plPollTimer = null;
// The waveform behind the seek bar (#101): the buckets themselves, and which
// track they are for. The backend decodes the file to get them, which takes
// long enough to matter, so an answer that arrives after the track has moved on
// is thrown away rather than painted over the wrong track.
let wavePeaks = null;
let waveFor = null;
// Where the playhead is, kept because the canvas is redrawn on resize and on a
// drag, not only when a status poll brings a new position.
let plPosition = 0;

/// How opaque the part of the track that has not played yet is drawn.
const UNPLAYED_ALPHA = 0.5;

// Ask for the waveform of `path`, and paint it if it is still the track playing
// when it arrives. A failure is silent: a missing picture is cosmetic, and a
// file that cannot be decoded here is one the player has already complained
// about.
async function loadWaveform(path) {
  waveFor = path;
  try {
    const peaks = await invoke("waveform", { path });
    if (waveFor !== path || playingPath !== path) return;
    wavePeaks = peaks;
    drawWave();
  } catch {
    if (waveFor === path) wavePeaks = null;
  }
}

// A different track is now the current one: drop the old picture before asking
// for the new one, so the bar is never showing one track's envelope under
// another's playhead.
//
// Called from BOTH places a track can change, which is not one place: starting
// one from the UI sets `playingPath` optimistically (see `playTrack`), so the
// poll's own change branch never fires for it and only sees the backend
// advancing the queue by itself.
function beginWaveform(path) {
  wavePeaks = null;
  plPosition = 0;
  drawWave();
  loadWaveform(path);
}

// One CSS custom property, resolved now. Read at draw time rather than cached:
// the theme can change under the player, and this runs a few times a second at
// most.
function themeColor(name, fallback) {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
}

// Paint the envelope, the played part of it, and the playhead.
//
// Mirrored around the middle, one bar every two device-independent pixels, each
// bar the loudest bucket it covers — so widening the window shows more detail
// and narrowing it never drops a peak. With no peaks (still decoding, or a file
// that would not decode) it draws the centre line alone, which reads as "no
// picture" rather than as an empty track.
function drawWave() {
  const wrap = plWave.parentElement;
  const width = Math.max(1, Math.round(wrap.clientWidth));
  const height = Math.max(1, Math.round(wrap.clientHeight));
  const dpr = window.devicePixelRatio || 1;
  if (plWave.width !== Math.round(width * dpr) || plWave.height !== Math.round(height * dpr)) {
    plWave.width = Math.round(width * dpr);
    plWave.height = Math.round(height * dpr);
  }
  const ctx = plWave.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);

  const played = plDuration > 0 ? Math.min(1, Math.max(0, plPosition / plDuration)) : 0;
  const middle = height / 2;
  // The unplayed half is drawn in the muted TEXT colour rather than the border
  // colour: a border is meant to be barely there, and at bar width it left the
  // rest of the track nearly invisible in both themes. Half opacity keeps it
  // quieter than the played part without disappearing into the panel.
  const behind = themeColor("--muted", "#636b76");
  const ahead = themeColor("--accent", "#0b6b53");

  if (!wavePeaks || !wavePeaks.length) {
    ctx.globalAlpha = UNPLAYED_ALPHA;
    ctx.fillStyle = behind;
    ctx.fillRect(0, middle - 0.5, width, 1);
    ctx.globalAlpha = 1;
    if (played > 0) {
      ctx.fillStyle = ahead;
      ctx.fillRect(0, middle - 0.5, width * played, 1);
    }
    return;
  }

  const pitch = 2;
  const bars = Math.max(1, Math.floor(width / pitch));
  const playedX = width * played;
  for (let bar = 0; bar < bars; bar += 1) {
    const from = Math.floor((bar * wavePeaks.length) / bars);
    const to = Math.max(from + 1, Math.floor(((bar + 1) * wavePeaks.length) / bars));
    let peak = 0;
    for (let i = from; i < to && i < wavePeaks.length; i += 1) {
      if (wavePeaks[i] > peak) peak = wavePeaks[i];
    }
    // A bar of at least one pixel each side of the middle, so a silent passage
    // is still a line rather than a gap in the bar.
    const half = Math.max(0.5, (peak / 255) * (height / 2 - 1));
    const x = bar * pitch;
    // A bar counts as played once the playhead has ENTERED it. Requiring the
    // whole bar to be behind the head left the one under it grey, which read as
    // the colour lagging a couple of pixels behind the cursor.
    const done = x < playedX;
    ctx.globalAlpha = done ? 1 : UNPLAYED_ALPHA;
    ctx.fillStyle = done ? ahead : behind;
    ctx.fillRect(x, middle - half, pitch - 1, half * 2);
  }
  ctx.globalAlpha = 1;
  // The playhead itself, so the boundary is readable even where the envelope is
  // flat and the two colours meet mid-bar.
  ctx.fillStyle = ahead;
  ctx.fillRect(Math.min(width - 1, playedX), 0, 1, height);
}

// ---- preview player ----
// Playback is native (rodio backend, #30): the UI sends commands and polls the
// backend's status. Gapless + auto-advance happen in the backend, which keeps
// the current + next track queued in one sink; the UI just feeds the next track
// whenever the current one changes.

function fmtTime(seconds) {
  if (!isFinite(seconds) || seconds < 0) seconds = 0;
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

// Stop/seek only make sense with a loaded track; the play/pause button stays
// enabled even when idle so it can start the current track (#99 redesign).
function setPlayerControlsEnabled(on) {
  plTitle.disabled = !on; // nothing loaded, nothing to find in the table
  plStop.disabled = !on;
  plSeek.disabled = !on;
  el("pl-prev").disabled = !on;
  el("pl-next").disabled = !on;
}

// Playback volume: a pure display-side preference like the theme/font ones, so
// it lives in localStorage and is pushed to the backend on startup and on every
// change (the audio thread keeps it across sink rebuilds — see player.rs). Mute
// remembers the pre-mute level so unmuting returns where you were.
const VOLUME_STORAGE_KEY = "tagrex.volume";
let volumeBeforeMute = 1;
function storedVolume() {
  try {
    const v = parseFloat(localStorage.getItem(VOLUME_STORAGE_KEY));
    if (Number.isFinite(v)) return Math.min(1, Math.max(0, v));
  } catch (e) {
    /* fall through to the default */
  }
  return 1;
}
function applyVolume(level, { persist = true } = {}) {
  const v = Math.min(1, Math.max(0, level));
  invoke("player_set_volume", { level: v });
  el("pl-volume").value = String(Math.round(v * 100));
  el("pl-mute").innerHTML = ico(v === 0 ? "volume-off" : "volume");
  const label = v === 0 ? "Unmute" : "Mute";
  el("pl-mute").title = label;
  el("pl-mute").setAttribute("aria-label", label);
  // The button on the bar carries the same glyph: the slider is folded away, so
  // this is the only place muted can be read without opening anything.
  el("pl-volume-btn").innerHTML = ico(v === 0 ? "volume-off" : "volume");
  const barLabel = v === 0 ? "Volume — muted" : `Volume — ${Math.round(v * 100)}%`;
  el("pl-volume-btn").title = barLabel;
  el("pl-volume-btn").setAttribute("aria-label", barLabel);
  if (v > 0) volumeBeforeMute = v;
  if (persist) {
    try {
      localStorage.setItem(VOLUME_STORAGE_KEY, String(v));
    } catch (e) {
      /* localStorage unavailable — preference just won't persist */
    }
  }
}

// What the player row calls the current track. Tags beat the file name — the
// point of playing a track here is usually to check it against its tags, and
// "Wish Mountain — Radio" answers that where "102_wish_mountain_-_radio.mp3"
// doesn't. Falls back to the file name when the tags are empty or the row isn't
// in the current table (a filtered-out or already-closed library).
function playerLabel(path) {
  const t = tracks.find((x) => x.path === path);
  const artist = ((t && t.tags && t.tags.artist) || "").trim();
  const title = ((t && t.tags && t.tags.title) || "").trim();
  if (artist && title) return `${artist} — ${title}`;
  return title || fileName(path);
}

// Show/hide the player row as a unit. #31 kept the row on screen permanently so
// a Play control was always reachable, but that spent a whole footer row on a
// bar reading "No track loaded"; the row now appears only while a track is
// loaded, and the status bar carries the Play control the rest of the time, so
// #31's intent survives without the standing cost. The reveal animation lives in
// CSS and re-runs each time the row is displayed.
function setPlayerVisible(on) {
  playerBar.hidden = !on;
  el("sb-play").hidden = on;
}

// Arm the player (once a library is open): the status-bar Play control becomes
// available and status polling starts. The row itself stays down until a track
// is actually loaded.
function showPlayerBar() {
  playerIdle();
  plToggle.disabled = false; // usable even when idle: starts the current track
  if (!plPollTimer) plPollTimer = setInterval(pollPlayerStatus, 300);
}

// Reset the UI to its idle, no-track state: controls disabled, placeholder
// title, zeroed time. The bar stays visible (#31). Used on stop, end of list,
// and when opening a library.
function playerIdle() {
  playingPath = null;
  plPaused = false;
  plDuration = 0;
  plTitle.textContent = t("player.noTrack");
  plTitle.title = "";
  plTime.textContent = "0:00 / 0:00";
  plSeek.value = "0";
  plToggle.innerHTML = ico("play");
  // Nothing loaded, nothing to draw (#101).
  wavePeaks = null;
  waveFor = null;
  plPosition = 0;
  drawWave();
  clearPlayerCover(); // #275
  playerBar.classList.add("idle");
  setPlayerVisible(false);
  setPlayerControlsEnabled(false);
  markPlayingRow();
}

// The path of the next visible row after `path` in the current table order
// (respecting sort/filter/manual reorder), or null if `path` is the last one.
// The table is windowed (#189), so the rows are no longer the list — the
// renderer's model is, and it already excludes group headers (which carry no
// path) and the files inside a collapsed folder.
function stepVisiblePath(path, delta) {
  const paths = hooks.navigablePaths();
  const i = paths.indexOf(path);
  if (i < 0) return null;
  return paths[i + delta] ?? null;
}
function nextVisiblePath(path) {
  return stepVisiblePath(path, 1);
}
function prevVisiblePath(path) {
  return stepVisiblePath(path, -1);
}
function firstVisiblePath() {
  return hooks.navigablePaths()[0] ?? null;
}

// Repeat: off / all (wrap at the end of the list) / one (loop this track). It
// works by changing what gets primed as the gapless "next" rather than by
// intercepting the end of playback, so the backend queue stays the single
// mechanism for continuing.
const REPEAT_STORAGE_KEY = "tagrex.repeat";
const REPEAT_MODES = ["off", "all", "one"];
let repeatMode = (() => {
  try {
    const v = localStorage.getItem(REPEAT_STORAGE_KEY);
    return REPEAT_MODES.includes(v) ? v : "off";
  } catch (e) {
    return "off";
  }
})();
function queuedAfter(path) {
  if (repeatMode === "one") return path;
  return nextVisiblePath(path) || (repeatMode === "all" ? firstVisiblePath() : null);
}
function applyRepeatMode(mode) {
  repeatMode = REPEAT_MODES.includes(mode) ? mode : "off";
  const btn = el("pl-repeat");
  btn.classList.toggle("active", repeatMode !== "off");
  btn.innerHTML = ico("repeat") + (repeatMode === "one" ? `<span class="pl-repeat-one">1</span>` : "");
  const label =
    repeatMode === "off"
      ? "Repeat off"
      : repeatMode === "all"
        ? "Repeat all"
        : "Repeat this track";
  btn.title = label;
  btn.setAttribute("aria-label", label);
  try {
    localStorage.setItem(REPEAT_STORAGE_KEY, repeatMode);
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

// Start playing `path`. Clicking the already-current track toggles play/pause.
// Also primes the next visible track so the backend can play it gaplessly.
// The now-playing cover (#275). Reading it is async and a fast Next/auto-advance
// can start a second read before the first returns, so a generation token drops
// any answer that a newer track has superseded — the bar never shows the cover of
// a track that is no longer playing.
let coverGen = 0;
async function setPlayerCover(path) {
  const gen = ++coverGen;
  try {
    // The track's EMBEDDED front cover, via the same call the editor's cover well
    // uses (read_cover_summary) — read_cover_image reads an external image file to
    // embed, not a track's own art, so it never returned the playing track's cover.
    const summary = await invoke("read_cover_summary", { paths: [path] });
    if (gen !== coverGen) return; // a newer track took over
    const front = summary && summary.samples && summary.samples[0];
    if (front && front.data_base64) {
      plCoverImg.src = `data:${front.mime};base64,${front.data_base64}`;
      plCover.classList.add("has-art");
      return;
    }
  } catch (e) {
    if (gen !== coverGen) return;
  }
  clearPlayerCover();
}
function clearPlayerCover() {
  coverGen++; // cancel any in-flight read
  plCover.classList.remove("has-art");
  plCoverImg.removeAttribute("src");
}

function playTrack(path) {
  if (path === playingPath) {
    togglePlay();
    return;
  }
  invoke("player_play", { path });
  // No eager priming: the backend raises wants_next near the end of the track
  // and the poll answers it then, so a Repeat/queue change made mid-track still
  // decides what plays next (an appended source can't be taken back).
  // Optimistic UI; the next poll confirms from the backend.
  playingPath = path;
  plPaused = false;
  plTitle.textContent = playerLabel(path);
  plTitle.title = `${path}\nClick to find this track in the table`;
  playerBar.classList.remove("idle");
  setPlayerVisible(true);
  setPlayerControlsEnabled(true);
  markPlayingRow();
  beginWaveform(path); // #101 — and this is the path most tracks start on
  setPlayerCover(path); // #275
}

function togglePlay() {
  if (!playingPath) return;
  plPaused = !plPaused;
  invoke(plPaused ? "player_pause" : "player_resume", {});
  markPlayingRow();
}

// Manual stop returns the bar to its idle state (still visible, #31).
function stopPlayback() {
  invoke("player_stop", {});
  playerIdle();
}

// Reflect the active track + play/pause state in the table without a full
// re-render (which would drop pending edits mid-typing).
function markPlayingRow() {
  tracksBody.querySelectorAll("tr").forEach((tr) => {
    tr.classList.toggle("playing", tr.dataset.path === playingPath);
  });
  plToggle.innerHTML = ico(playingPath && !plPaused ? "pause" : "play");
}

// Poll the backend and mirror its state. When the current track changes (a
// gapless transition, i.e. auto-advance #29), update the UI and feed the next
// track; when it wants a next track but none is queued, feed it too.
async function pollPlayerStatus() {
  let st;
  try {
    st = await invoke("player_status", {});
  } catch (e) {
    return;
  }
  const changed = st.path !== playingPath;
  playingPath = st.path;
  plPaused = st.is_paused;

  if (!st.path) {
    // Backend drained (end of list or stopped): go idle unless already idle.
    if (!playerBar.classList.contains("idle")) playerIdle();
    return;
  }

  if (changed) {
    plTitle.textContent = playerLabel(st.path);
    plTitle.title = `${st.path}\nClick to find this track in the table`;
    playerBar.classList.remove("idle");
    setPlayerVisible(true);
    setPlayerControlsEnabled(true);
    markPlayingRow();
    beginWaveform(st.path); // the backend advanced the queue on its own (#101)
    setPlayerCover(st.path); // #275
  }
  // Keep the queue primed for gapless continuation.
  if (st.wants_next) {
    const next = queuedAfter(st.path);
    if (next) invoke("player_set_next", { path: next });
  }
  // A decoder that refuses to seek leaves the clock where the audio actually is
  // (#190), so the bar snaps back on the next poll by itself — all that is
  // missing is why, said once per track rather than on every poll.
  if (st.seek_refused && seekRefusedFor !== st.path) {
    seekRefusedFor = st.path;
    toast(t("toast.player.noSeek"), true);
  } else if (!st.seek_refused && seekRefusedFor === st.path) {
    seekRefusedFor = null;
  }
  plDuration = st.duration_secs || 0;
  if (!plSeeking) {
    plPosition = st.position_secs || 0;
    plSeek.value = plDuration
      ? String(Math.round((st.position_secs / plDuration) * 1000))
      : "0";
    drawWave();
  }
  plTime.textContent = `${fmtTime(st.position_secs)} / ${fmtTime(plDuration)}`;
  plToggle.innerHTML = ico(plPaused ? "play" : "pause");
}

// The track the bottom Play button starts when nothing is playing: the active
// (last-clicked / keyboard) row, else the first selected, else the top of the
// list — then the backend auto-advances down the list to the end (#99 redesign,
// the per-row play button was removed).
function currentPlayTarget() {
  const paths = hooks.navigablePaths();
  if (!paths.length) return null;
  if (activeRowPath && paths.includes(activeRowPath)) return activeRowPath;
  return paths.find((path) => selection.has(path)) ?? paths[0];
}

function playPauseFromBar() {
  // While actually playing, the button is a pause button — pause the current
  // track (don't jump to whatever row is selected).
  if (playingPath && !plPaused) {
    togglePlay();
    return;
  }
  // Paused or idle, the button is a play button: play the current target.
  // playTrack() resumes when the target IS the paused track, and switches to it
  // otherwise — so pausing A, selecting B, then Play now plays B (not A).
  const path = currentPlayTarget();
  if (path) playTrack(path);
  else if (playingPath) togglePlay();
}

plToggle.addEventListener("click", playPauseFromBar);
el("sb-play").addEventListener("click", playPauseFromBar);
plStop.addEventListener("click", stopPlayback);
// While dragging, show the target time locally and suppress poll overrides;
// commit the seek to the backend on release.
plSeek.addEventListener("input", () => {
  plSeeking = true;
  const target = (Number(plSeek.value) / 1000) * plDuration;
  plTime.textContent = `${fmtTime(target)} / ${fmtTime(plDuration)}`;
  // The playhead follows the drag rather than waiting for the release: the
  // point of a waveform is aiming at something you can see.
  plPosition = target;
  drawWave();
});
// Prev/Next step through the same playable files the gapless queue uses. At an
// end they wrap only when Repeat all is on, matching what auto-advance does.
el("pl-prev").addEventListener("click", () => {
  if (!playingPath) return;
  const target =
    prevVisiblePath(playingPath) || (repeatMode === "all" ? hooks.navigablePaths().at(-1) ?? null : null);
  if (target) playTrack(target);
});
el("pl-next").addEventListener("click", () => {
  if (!playingPath) return;
  const target = nextVisiblePath(playingPath) || (repeatMode === "all" ? firstVisiblePath() : null);
  if (target) playTrack(target);
});
el("pl-repeat").addEventListener("click", () => {
  applyRepeatMode(REPEAT_MODES[(REPEAT_MODES.indexOf(repeatMode) + 1) % REPEAT_MODES.length]);
});
el("pl-volume").addEventListener("input", (e) => {
  applyVolume(Number(e.target.value) / 100);
});

// ---- the volume popover ----
// The slider held a fixed strip of the row to be used for a few seconds at a
// time, and the row is where the waveform wants every pixel it can get. Folded
// behind its own button, placed with the same helper every other popover uses.
const volumePop = el("pl-volume-pop");

function closeVolume() {
  volumePop.hidden = true;
  el("pl-volume-btn").setAttribute("aria-expanded", "false");
}

function toggleVolume() {
  if (!volumePop.hidden) {
    closeVolume();
    return;
  }
  volumePop.hidden = false;
  el("pl-volume-btn").setAttribute("aria-expanded", "true");
  placeFloating(volumePop, el("pl-volume-btn"), { align: "right" });
  el("pl-volume").focus();
}

// The title answers "which row is this?" — a real question after a few minutes
// of listening, and the bar is the only thing that knows. Selects the track and
// scrolls it into view; says so when the file is not in the open library, which
// happens after opening a different folder while playback carries on.
plTitle.addEventListener("click", () => {
  if (!playingPath) return;
  if (!hooks.revealPath(playingPath)) {
    toast(t("toast.player.gone"));
  }
});

el("pl-volume-btn").addEventListener("click", (e) => {
  e.stopPropagation();
  toggleVolume();
});
document.addEventListener("mousedown", (e) => {
  if (!volumePop.hidden && !e.target.closest(".pl-vol")) closeVolume();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !volumePop.hidden) {
    closeVolume();
    el("pl-volume-btn").focus();
  }
});
window.addEventListener("resize", () => {
  if (!volumePop.hidden) placeFloating(volumePop, el("pl-volume-btn"), { align: "right" });
});
el("pl-mute").addEventListener("click", () => {
  const cur = Number(el("pl-volume").value) / 100;
  applyVolume(cur > 0 ? 0 : volumeBeforeMute || 1);
});
plSeek.addEventListener("change", () => {
  const secs = (Number(plSeek.value) / 1000) * plDuration;
  invoke("player_seek", { secs });
  plSeeking = false;
});

// Whether `path` is the track the backend reports as playing. The file table
// tints that row as it renders, and the transport state is this module's.
function isPlayingPath(path) {
  return path === playingPath;
}

// A canvas keeps its pixels, not its layout, so the picture is redrawn whenever
// the bar's width changes. An observer on the element rather than a window
// resize listener: the bar also grows when the mode panel is collapsed or the
// splitter is dragged, and neither of those is a window resize.

if (window.ResizeObserver) {
  new ResizeObserver(() => drawWave()).observe(plWave.parentElement);
} else {
  window.addEventListener("resize", drawWave);
}

// The transport's own persisted state, restored as the module loads — it used
// to sit in app.js's start-up block, but nothing outside needs to know that a
// repeat mode or a volume was remembered.
applyRepeatMode(repeatMode);
applyVolume(storedVolume(), { persist: false });

export { fmtTime, showPlayerBar, playTrack, isPlayingPath };
