// The About dialog (#805): the version and the project's links. Opened from the
// wordmark in the top bar, from the Settings footer and, on macOS, from the app
// menu, so it is there on every platform. The links name a place
// and the backend picks the address, so nothing here can open anything else.
import { el, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { t } from "./i18n.js";

let version = "";

// Asked once: the version cannot change while the app runs.
async function loadVersion() {
  if (version) return version;
  try {
    version = (await invoke("app_version", {})) || "";
  } catch (e) {
    version = "";
  }
  return version;
}

function paintVersion() {
  el("about-version").textContent = version ? t("about.version", { version }) : "";
  // A name and a number, the same in every language, so a language switch
  // leaves nothing stale here.
  el("about-settings-open").textContent = version ? `TagRex ${version}` : "TagRex";
}

export async function openAbout() {
  await loadVersion();
  paintVersion();
  el("about-modal").hidden = false;
  el("about-close").focus();
}

function closeAbout() {
  el("about-modal").hidden = true;
}

// The Settings footer shows the version too, filled once at start.
export async function paintAboutVersion() {
  await loadVersion();
  paintVersion();
}

export function initAbout() {
  el("brand").addEventListener("click", openAbout);
  el("about-settings-open").addEventListener("click", openAbout);
  el("about-close").addEventListener("click", closeAbout);
  const modal = el("about-modal");
  modal.addEventListener("mousedown", (e) => {
    if (e.target === modal) closeAbout();
  });
  // Escape closes this dialog only: opened from Settings, the sheet stays.
  modal.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    closeAbout();
  });
  modal.querySelectorAll("[data-about-link]").forEach((button) => {
    button.addEventListener("click", () => {
      invoke("open_about_link", { link: button.dataset.aboutLink }).catch((e) => toast(String(e), true));
    });
  });
  // The macOS app menu's About item (#805) opens this dialog, not a native one.
  const events = window.__TAURI__ && window.__TAURI__.event;
  if (events) events.listen("tagrex://about", openAbout);
  paintAboutVersion();
}
