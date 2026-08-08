// Bridge to the Rust command layer. Inside the Tauri webview this is the real
// IPC; in a plain browser (used to develop/verify the UI) it falls back to a
// small in-memory mock so the interface can be exercised without the native
// shell.
import { mockInvoke } from "./mock.js";

export const TAURI = window.__TAURI__ ? window.__TAURI__.core : null;

export async function invoke(cmd, args) {
  if (TAURI) return TAURI.invoke(cmd, args);
  return mockInvoke(cmd, args);
}
