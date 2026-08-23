// Bridge to the Rust command layer. Inside the Tauri webview this is the real
// IPC; in a plain browser (used to develop/verify the UI) it falls back to a
// small in-memory mock so the interface can be exercised without the native
// shell.
import { mockInvoke } from "./mock.js";
import { message } from "./i18n.js";

export const TAURI = window.__TAURI__ ? window.__TAURI__.core : null;

// A failure the backend describes as a code and its values (#268), wrapped so
// it reads as a sentence wherever one is expected.
//
// Every caller already does `toast(String(e), true)`; `toString` is what makes
// that keep working and become translated at the same time. It renders at the
// moment the message is shown, not when the failure happened, so a language
// switched in between is the one the reader gets.
class BackendError extends Error {
  constructor(failure) {
    super(failure && failure.text ? failure.text : String(failure));
    this.name = "BackendError";
    this.failure = failure;
  }

  toString() {
    if (!this.failure || typeof this.failure !== "object") return String(this.failure);
    return message(this.failure.message) || this.failure.text || "";
  }
}

export async function invoke(cmd, args) {
  try {
    return await (TAURI ? TAURI.invoke(cmd, args) : mockInvoke(cmd, args));
  } catch (failure) {
    // The mock rejects the same shapes the backend does — including the plain
    // strings it still uses for the failures it invents itself — so the browser
    // path exercises this and not a simpler one.
    throw new BackendError(failure);
  }
}
