// A cover at full size (#811): a click on a cover thumbnail opens it over the
// window, fitted to it, with its pixel size, format and file size underneath.
// The image grows out of the thumbnail and shrinks back into it, unless the
// system asks for reduced motion.
//
// The viewer is a `.modal-backdrop`, which is what the Esc-discards-the-plan
// handler (#533) checks for, so Escape here closes the viewer and nothing
// else; its own handler stops the key before the search-stop one sees it.
import { el } from "./dom.js";
import { t } from "./i18n.js";

const MOTION_MS = 220;
let origin = null; // the thumbnail the open image came from
let closing = false;

const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// The bytes behind a base64 data URI, without decoding it.
function dataUriBytes(src) {
  const comma = src.indexOf(",");
  if (!src.startsWith("data:") || comma < 0) return 0;
  const body = src.slice(comma + 1);
  const padding = body.endsWith("==") ? 2 : body.endsWith("=") ? 1 : 0;
  return Math.max(0, Math.floor((body.length * 3) / 4) - padding);
}

function formatName(src) {
  const match = /^data:image\/([a-z0-9.+-]+)/i.exec(src);
  if (!match) return "";
  const kind = match[1].toLowerCase();
  return kind === "svg+xml" ? "SVG" : kind.toUpperCase();
}

function sizeText(bytes) {
  if (!bytes) return "";
  if (bytes < 1024) return t("coverView.bytes", { n: bytes });
  if (bytes < 1024 * 1024) return t("coverView.kb", { n: Math.round(bytes / 1024) });
  return t("coverView.mb", { n: (bytes / (1024 * 1024)).toFixed(1) });
}

// The transform that puts the full image where `from` is drawn.
function poseOver(image, from) {
  const to = image.getBoundingClientRect();
  if (!from || !from.width || !to.width) return "translate(0px, 8px) scale(0.96)";
  const x = from.left + from.width / 2 - (to.left + to.width / 2);
  const y = from.top + from.height / 2 - (to.top + to.height / 2);
  return `translate(${x}px, ${y}px) scale(${from.width / to.width}, ${from.height / to.height})`;
}

// Fit the image to the window, small covers enlarged too: the line under it
// gives the real pixel size, so the enlargement hides nothing.
function fitImage() {
  const image = el("cover-view-img");
  const width = image.naturalWidth;
  const height = image.naturalHeight;
  if (!width || !height) return;
  const scale = Math.min((window.innerWidth - 96) / width, (window.innerHeight - 136) / height);
  image.style.width = `${Math.max(1, Math.floor(width * scale))}px`;
  image.style.height = `${Math.max(1, Math.floor(height * scale))}px`;
}

// The thumbnail's box while it is on screen, else null (scrolled away, or
// re-rendered since the viewer opened).
function originRect() {
  if (!origin || !origin.isConnected) return null;
  const rect = origin.getBoundingClientRect();
  return rect.width && rect.height ? rect : null;
}

export async function openCoverView(thumb) {
  if (!thumb || !thumb.src) return;
  const view = el("cover-view");
  const image = el("cover-view-img");
  origin = thumb;
  closing = false;
  image.src = thumb.src;
  image.alt = thumb.alt || "";
  try {
    await image.decode();
  } catch (e) {
    /* drawn as far as the browser can; the size line just stays short */
  }
  const parts = [];
  if (image.naturalWidth) parts.push(`${image.naturalWidth} × ${image.naturalHeight}`);
  const format = formatName(thumb.src);
  if (format) parts.push(format);
  const size = sizeText(dataUriBytes(thumb.src));
  if (size) parts.push(size);
  el("cover-view-meta").textContent = parts.join(" · ");
  fitImage();
  view.hidden = false;
  el("cover-view-close").focus({ preventScroll: true });
  if (reducedMotion() || !image.animate) return;
  view.animate([{ opacity: 0 }, { opacity: 1 }], { duration: MOTION_MS, easing: "ease-out" });
  image.animate([{ transform: poseOver(image, originRect()) }, { transform: "none" }], {
    duration: MOTION_MS,
    easing: "cubic-bezier(0.2, 0.8, 0.2, 1)",
  });
}

export async function closeCoverView() {
  const view = el("cover-view");
  if (view.hidden || closing) return;
  closing = true;
  const image = el("cover-view-img");
  if (!reducedMotion() && image.animate) {
    // From wherever an unfinished opening has got to, back into the thumbnail.
    const current = getComputedStyle(image).transform;
    image.getAnimations().forEach((animation) => animation.cancel());
    const back = image.animate(
      [{ transform: current === "none" ? "none" : current }, { transform: poseOver(image, originRect()) }],
      { duration: MOTION_MS, easing: "cubic-bezier(0.4, 0, 0.6, 1)", fill: "forwards" },
    );
    const fade = view.animate([{ opacity: 1 }, { opacity: 0 }], {
      duration: MOTION_MS,
      easing: "ease-in",
      fill: "forwards",
    });
    // A hidden or minimised window does not run animations, so the close never
    // waits on them for longer than they should take.
    await Promise.race([
      Promise.allSettled([back.finished, fade.finished]),
      new Promise((resolve) => setTimeout(resolve, MOTION_MS + 150)),
    ]);
    // Hidden first, then the held end frames dropped, so the full image is
    // never drawn again at full size on the way out.
    view.hidden = true;
    back.cancel();
    fade.cancel();
  } else {
    view.hidden = true;
  }
  image.removeAttribute("src");
  image.style.width = image.style.height = "";
  if (origin && origin.isConnected) origin.focus({ preventScroll: true });
  origin = null;
  closing = false;
}

export function initCoverView() {
  const view = el("cover-view");
  window.addEventListener("resize", () => {
    if (!view.hidden) fitImage();
  });
  view.addEventListener("click", () => closeCoverView());
  view.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    closeCoverView();
  });
}

// Make a cover thumbnail open the viewer: by click, and by Enter or Space once
// Tab has reached it.
export function makeViewable(thumb) {
  thumb.tabIndex = 0;
  thumb.setAttribute("role", "button");
  thumb.classList.add("viewable");
  thumb.title = t("coverView.open");
  thumb.addEventListener("click", (e) => {
    e.stopPropagation();
    openCoverView(thumb);
  });
  thumb.addEventListener("keydown", (e) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    e.preventDefault();
    e.stopPropagation();
    openCoverView(thumb);
  });
}
