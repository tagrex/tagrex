// The preview player (#30, #143 split it out of app.js).
//
// Playback is native (rodio backend): this module sends commands and polls the
// backend's status, and everything about the transport — the queued next track,
// repeat, volume, the seek drag — is its own business. The rest of the UI only
// asks it to start a track or show the bar.
import { el, fileName, ico, toast } from "./dom.js";
import { t } from "./i18n.js";
import { hooks } from "./hooks.js";
import { invoke } from "./invoke.js";
import { trackAt, selection, activeRowPath } from "./state.js";

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
// What the system's Now Playing was last told (#218): the track, its state and
// where playback was at that moment. The system runs its own clock from there,
// so it is told again only when the picture would otherwise go wrong.
let published = null;

// A native range keeps seeking keyboard-accessible without decoding the audio
// a second time. CSS draws the completed fraction in the current accent colour.
//
// The clock and the slider's accessible value are the same string and both only
// change when the whole second does. The poll runs three times a second, so
// every one of those writes was replacing a text node and an attribute with
// exactly what was already there (two of every three polls); `seekLabel` is the
// one place that answers "did the second actually change?".
let seekLabel = "";
function showSeekLabel(position) {
  const text = `${fmtTime(position)} / ${fmtTime(plDuration)}`;
  if (text === seekLabel) return;
  seekLabel = text;
  plTime.textContent = text;
  plSeek.setAttribute("aria-valuetext", text);
}
function updateSeekDisplay(position) {
  const fraction = plDuration > 0 ? Math.min(1, Math.max(0, position / plDuration)) : 0;
  plSeek.value = String(Math.round(fraction * 1000));
  plSeek.style.setProperty("--seek-progress", `${fraction * 100}%`);
  showSeekLabel(position);
}

// The play/pause glyph is the same between polls too. Rebuilding it with
// `innerHTML` every 300 ms threw away the <svg> and its <use> reference for no
// visible change; the href read makes the cache self-checking, so a bar whose
// markup was replaced from elsewhere is still repainted.
let toggleIcon = "";
function showToggleIcon(name) {
  const use = plToggle.firstElementChild?.firstElementChild;
  if (name === toggleIcon && use?.getAttribute("href") === `#i-${name}`) return;
  plToggle.innerHTML = ico(name);
  toggleIcon = name;
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
  const label = v === 0 ? t("player.unmute") : t("player.mute");
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
  const t = trackAt(path);
  const artist = ((t && t.tags && t.tags.artist) || "").trim();
  const title = ((t && t.tags && t.tags.title) || "").trim();
  if (artist && title) return `${artist} — ${title}`;
  return title || fileName(path);
}

// Arm the always-visible player once a library is open and start polling.
function showPlayerBar() {
  playerIdle();
  plToggle.disabled = !currentPlayTarget();
  if (!plPollTimer) plPollTimer = setInterval(pollPlayerStatus, 300);
}

// Reset the UI to its idle, no-track state: controls disabled, placeholder
// title, zeroed time. The bar stays visible (#31). Used on stop, end of list,
// and when opening a library.
function playerIdle() {
  playingPath = null;
  plPaused = false;
  plDuration = 0;
  plSeeking = false;
  plTitle.dataset.i18n = "player.noTrack";
  plTitle.textContent = t("player.noTrack");
  plTitle.title = "";
  // The idle clock and the disabled slider share one write path; the label is
  // "0:00 / 0:00" again only because plDuration is zero here.
  updateSeekDisplay(0);
  showToggleIcon("play");
  publishNowPlaying(0); // #218: nothing is playing any more
  playerBar.classList.add("idle");
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
      ? t("player.repeatOff")
      : repeatMode === "all"
        ? t("player.repeatAll")
        : t("player.repeatOne");
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
  plTitle.removeAttribute("data-i18n");
  plTitle.textContent = playerLabel(path);
  plTitle.title = `${path}\n${t("player.findTrack")}`;
  playerBar.classList.remove("idle");
  setPlayerControlsEnabled(true);
  markPlayingRow();
  updateSeekDisplay(0);
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
  showToggleIcon(playingPath && !plPaused ? "pause" : "play");
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
    plTitle.removeAttribute("data-i18n");
    plTitle.textContent = playerLabel(st.path);
    plTitle.title = `${st.path}\n${t("player.findTrack")}`;
    playerBar.classList.remove("idle");
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
    toast(t("toast.player.noSeek"), true);
  } else if (!st.seek_refused && seekRefusedFor === st.path) {
    seekRefusedFor = null;
  }
  plDuration = st.duration_secs || 0;
  // While the slider is being dragged the bar shows the drag target; a poll
  // landing mid-drag must not pull the clock back to where the backend still
  // is. Both the text and the slider's accessible value go through this one
  // call, so suppressing it suppresses them together.
  if (!plSeeking) updateSeekDisplay(st.position_secs || 0);
  showToggleIcon(plPaused ? "play" : "pause");
  if (nowPlayingStale(st.position_secs)) publishNowPlaying(st.position_secs);
}

// The system media session (#218): the Mac's media keys, the Now Playing widget
// and headphone controls. The system only sends those to the app that says what
// it is playing, so every change of track, state or position is reported here,
// from the same poll that keeps the bar in step.
function nowPlayingTags() {
  const track = trackAt(playingPath);
  const tags = (track && track.tags) || {};
  return {
    title: (tags.title || "").trim() || fileName(playingPath),
    artist: (tags.artist || "").trim(),
    album: (tags.album || "").trim(),
  };
}

function publishNowPlaying(position) {
  if (!playingPath) {
    if (published && published.path === null) return;
    published = { path: null };
    invoke("media_session_update", {
      now: { state: "stopped", path: null, title: "", artist: "", album: "", duration_secs: 0, position_secs: 0 },
    }).catch(() => {});
    return;
  }
  const tags = nowPlayingTags();
  published = {
    path: playingPath,
    paused: plPaused,
    duration: plDuration,
    label: `${tags.title}\n${tags.artist}\n${tags.album}`,
    position,
    at: performance.now(),
  };
  invoke("media_session_update", {
    now: {
      state: plPaused ? "paused" : "playing",
      path: playingPath,
      ...tags,
      duration_secs: plDuration,
      position_secs: position,
    },
  }).catch(() => {});
}

// Whether the system's picture has drifted from the player's: another track, a
// pause, a length that was not known yet, edited tags, or a position that jumped
// — a seek, from the bar or from the system's own widget.
function nowPlayingStale(position) {
  if (!published || published.path !== playingPath) return true;
  if (published.paused !== plPaused || published.duration !== plDuration) return true;
  const tags = nowPlayingTags();
  if (published.label !== `${tags.title}\n${tags.artist}\n${tags.album}`) return true;
  const expected = published.position + (plPaused ? 0 : (performance.now() - published.at) / 1000);
  return Math.abs(position - expected) > 2;
}

// A remote command from the system, handled as the matching control would be —
// except that Play resumes the paused track. The bar's button plays the selected
// row when paused, which is a choice made on screen; a media key is pressed
// without looking, and after Next the selection is still the first track.
function onMediaControl(control) {
  switch (control && control.action) {
    case "toggle":
      if (playingPath) togglePlay();
      else playPauseFromBar();
      break;
    case "play":
      if (playingPath && plPaused) togglePlay();
      else if (!playingPath) playPauseFromBar();
      break;
    case "pause":
      if (playingPath && !plPaused) togglePlay();
      break;
    case "stop":
      if (playingPath) stopPlayback();
      break;
    case "next":
      el("pl-next").click();
      break;
    case "previous":
      el("pl-prev").click();
      break;
    case "seek":
      if (playingPath && Number.isFinite(control.position_secs)) {
        seekTo(control.position_secs);
      }
      break;
  }
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
plStop.addEventListener("click", stopPlayback);
// While dragging, show the target time locally and suppress poll overrides;
// commit the seek to the backend on release.
plSeek.addEventListener("input", () => {
  plSeeking = true;
  const target = (Number(plSeek.value) / 1000) * plDuration;
  updateSeekDisplay(target);
});
// A cancelled gesture or a return to the original value may omit `change`.
// Resume polling on release/focus loss too, without sending an extra seek.
function endSeekGesture() { plSeeking = false; }
window.addEventListener("pointerup", endSeekGesture, true);
window.addEventListener("pointercancel", endSeekGesture, true);
window.addEventListener("blur", endSeekGesture);
plSeek.addEventListener("blur", endSeekGesture);
const SEEK_RELEASE_KEYS = new Set(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"]);
plSeek.addEventListener("keyup", (event) => {
  if (SEEK_RELEASE_KEYS.has(event.key)) endSeekGesture();
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

el("pl-mute").addEventListener("click", () => {
  const cur = Number(el("pl-volume").value) / 100;
  applyVolume(cur > 0 ? 0 : volumeBeforeMute || 1);
});
plSeek.addEventListener("change", () => {
  const secs = (Number(plSeek.value) / 1000) * plDuration;
  seekTo(secs);
  endSeekGesture();
});

// How close to the end the backend starts asking for the next track
// (`PRIME_LEAD_SECS` in crates/player).
const PRIME_LEAD_SECS = 5;

// Seek within the current track (#528). The next track is queued when a status
// poll sees the end coming, and a seek into the last fraction of a second — the
// progress bar's last slice — can end the track before that poll comes round, which
// left nothing to play and stopped. So a seek that close to the end queues the
// next track itself first; the backend ignores it when one is already queued.
async function seekTo(secs) {
  if (playingPath && plDuration && plDuration - secs <= PRIME_LEAD_SECS) {
    const next = queuedAfter(playingPath);
    if (next) await invoke("player_set_next", { path: next });
  }
  invoke("player_seek", { secs });
}

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
playerIdle();

const tauriEvents = window.__TAURI__ && window.__TAURI__.event;
if (tauriEvents) tauriEvents.listen("tagrex://media-control", (e) => onMediaControl(e.payload));

export { fmtTime, showPlayerBar, playTrack, isPlayingPath };
