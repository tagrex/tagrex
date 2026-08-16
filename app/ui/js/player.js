// The preview player (#30, #143 split it out of app.js).
//
// Playback is native (rodio backend): this module sends commands and polls the
// backend's status, and everything about the transport — the queued next track,
// repeat, volume, the seek drag — is its own business. The rest of the UI only
// asks it to start a track or show the bar.
import { el, fileName, ico, toast } from "./dom.js";
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
  plTitle.textContent = "No track loaded";
  plTitle.title = "";
  plTime.textContent = "0:00 / 0:00";
  plSeek.value = "0";
  plToggle.innerHTML = ico("play");
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
  plTitle.title = path;
  playerBar.classList.remove("idle");
  setPlayerVisible(true);
  setPlayerControlsEnabled(true);
  markPlayingRow();
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
    plTitle.title = st.path;
    playerBar.classList.remove("idle");
    setPlayerVisible(true);
    setPlayerControlsEnabled(true);
    markPlayingRow();
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
    toast("This file can't be seeked", true);
  } else if (!st.seek_refused && seekRefusedFor === st.path) {
    seekRefusedFor = null;
  }
  plDuration = st.duration_secs || 0;
  if (!plSeeking) {
    plSeek.value = plDuration
      ? String(Math.round((st.position_secs / plDuration) * 1000))
      : "0";
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

// The transport's own persisted state, restored as the module loads — it used
// to sit in app.js's start-up block, but nothing outside needs to know that a
// repeat mode or a volume was remembered.
applyRepeatMode(repeatMode);
applyVolume(storedVolume(), { persist: false });

export { fmtTime, showPlayerBar, playTrack, isPlayingPath };
