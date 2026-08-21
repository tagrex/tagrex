// Dropping folders and files onto the window (#127, #143 split it out of
// app.js).
//
// Tauri v2 intercepts OS file drops (dragDropEnabled) and re-emits them as
// window events carrying absolute paths, so this listens for those rather than
// HTML5 file DnD, which the webview suppresses. A lone dropped image is a cover
// (#133) — an image can't be "opened", so where it lands doesn't matter.
import { hooks } from "./hooks.js";
import { embedCoverFromPath } from "./cover.js";
import { invoke } from "./invoke.js";

// ---- drag-and-drop onto the window to open folders/files (#127) ----
// Tauri v2 intercepts OS file drops (dragDropEnabled) and re-emits them as
// window events carrying absolute paths, so we listen for those rather than
// HTML5 file DnD (which the webview suppresses). Enter/over/leave toggle the
// drop-cue overlay; the drop hands the paths to the backend resolver.
function showDropCue(on) {
  document.body.classList.toggle("drag-active", on);
}

function isImagePath(p) {
  return /\.(jpe?g|png|webp|gif|bmp|tiff?|avif|heic)$/i.test(p);
}

(function initWindowDrop() {
  const event = window.__TAURI__ && window.__TAURI__.event;
  if (event) {
    event.listen("tauri://drag-enter", () => showDropCue(true));
    event.listen("tauri://drag-over", () => showDropCue(true));
    event.listen("tauri://drag-leave", () => showDropCue(false));
    event.listen("tauri://drag-drop", (e) => {
      showDropCue(false);
      const paths = (e && e.payload && e.payload.paths) || [];
      // A single dropped image has only one meaning — embed it as the cover of
      // the selection (#133). No position hit-test: an image can't be "opened"
      // as a library, so this is unambiguous and doesn't depend on fragile
      // physical/logical-pixel coordinate conversion. Everything else opens.
      if (paths.length === 1 && isImagePath(paths[0])) {
        embedCoverFromPath(paths[0]);
        return;
      }
      hooks.openDrop(paths);
    });
    return;
  }
  // Browser dev (no native shell): the OS can't hand us real paths, but wiring
  // HTML5 DnD still lets the overlay and open flow be exercised against the
  // mock. Drops on the cover well keep their own handler.
  window.addEventListener("dragover", (e) => {
    if (e.target.closest("#cover-well")) return;
    e.preventDefault();
    showDropCue(true);
  });
  window.addEventListener("dragleave", (e) => {
    if (e.relatedTarget === null) showDropCue(false);
  });
  window.addEventListener("drop", (e) => {
    if (e.target.closest("#cover-well")) return;
    e.preventDefault();
    showDropCue(false);
    hooks.openDrop(Array.from(e.dataTransfer.files).map((f) => f.name));
  });
})();

// ---- folders the OS hands over, rather than the window (#51) ----
// A folder picked with "Open With", dropped on the Dock icon or passed to a
// second launch reaches the backend, not the webview — as an Apple Event on
// macOS, as an argument everywhere else. Either way it ends up in the same
// place a drop does, so the paths are opened by the same call.
//
// The backend queues them and only nudges, because the hand-over can happen
// before this listener exists; draining is what makes the two paths safe to
// have both — whichever asks first gets the paths, and the other gets nothing.
async function openHandedOverPaths() {
  try {
    const paths = await invoke("take_launch_paths");
    if (paths && paths.length) await hooks.openDrop(paths);
  } catch {
    // Nothing was waiting, or there is no backend to ask (browser dev).
  }
}

// Called once from the startup sequence, after `hooks.openDrop` is wired. The
// listener goes on before the queue is drained, not after: registering it is
// itself async, and a hand-over that lands in between would nudge nobody and
// then find the queue already emptied.
export async function initLaunchOpen() {
  const event = window.__TAURI__ && window.__TAURI__.event;
  if (event) await event.listen("tagrex://open-paths", openHandedOverPaths);
  await openHandedOverPaths();
}
