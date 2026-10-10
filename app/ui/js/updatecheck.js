// Telling the user a newer release exists (#435): a quiet note in the status bar
// and a "Check now" in Settings. Notify only; the backend does the asking and the
// note opens the releases page in the browser.
import { el } from "./dom.js";
import { invoke } from "./invoke.js";
import { t } from "./i18n.js";

const SKIPPED_KEY = "tagrex.update.skipped";
// Long enough that it never competes with opening a folder.
const START_DELAY_MS = 4000;

function skippedVersion() {
  try {
    return localStorage.getItem(SKIPPED_KEY) || "";
  } catch (e) {
    return "";
  }
}

function showPill(latest) {
  el("update-open").textContent = t("update.available", { version: latest });
  el("update-pill").dataset.version = latest;
  el("update-pill").hidden = false;
}

export function initUpdateCheck() {
  el("update-open").addEventListener("click", () => invoke("open_releases_page", {}));
  el("update-skip").addEventListener("click", () => {
    try {
      localStorage.setItem(SKIPPED_KEY, el("update-pill").dataset.version || "");
    } catch (e) {
      /* the note simply comes back next time */
    }
    el("update-pill").hidden = true;
  });
  // Quiet by design: a failed or refused check says nothing. Only the button in
  // Settings reports how it went.
  setTimeout(async () => {
    try {
      const result = await invoke("check_for_update", { automatic: true });
      if (result && result.newer && result.latest !== skippedVersion()) showPill(result.latest);
    } catch (e) {
      /* offline, rate-limited, behind a proxy that says no */
    }
  }, START_DELAY_MS);
}

// The Settings button: asks regardless of the switch, and says what it found.
export async function checkUpdateNow() {
  const out = el("update-result");
  out.textContent = t("update.checking");
  try {
    const result = await invoke("check_for_update", { automatic: false });
    if (result && result.newer) {
      out.textContent = t("update.available", { version: result.latest });
      showPill(result.latest);
    } else if (result) {
      out.textContent = t("update.upToDate", { version: result.current });
    }
  } catch (e) {
    out.textContent = String(e);
  }
}
