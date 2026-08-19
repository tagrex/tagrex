// Field locks (#48).
//
// A lock marks a field as not-to-be-touched, and every plan the backend builds
// drops changes to it. This module owns the UI half: which fields are locked,
// the padlock that toggles one, and telling the rest of the interface to catch
// up when that changes.
//
// **The backend is what enforces the lock; this is the affordance.** The set is
// pushed to the session with `set_locked_fields` and the plan gate there is what
// actually keeps a locked field out of a change — so a locked cell being inert
// in the table is a courtesy, not the guarantee. That split matters: the table
// is one of several ways a change can start, and only one of them is the table.
//
// Locks last for the session and are deliberately not persisted. A lock set
// months ago and long forgotten is worse than no lock, because it makes an
// operation quietly do less than it says it does — so the app forgets them when
// it closes, and the only lock in force is one somebody set today.
import { ico } from "./dom.js";
import { hooks } from "./hooks.js";
import { invoke } from "./invoke.js";

// Storage keys currently locked. Read through `isFieldLocked`, never mutated
// from outside — the backend has to be told about every change, and one place
// doing the telling is what keeps the two from drifting.
const locked = new Set();

export function isFieldLocked(key) {
  return locked.has(key);
}

export function anyFieldLocked() {
  return locked.size > 0;
}

export function lockedFieldKeys() {
  return [...locked];
}

// Hand the session what is locked. Called after every toggle and again after a
// library is opened: opening builds a new session with nothing locked, and the
// locks belong to the person rather than to the folder they happen to have open.
export async function pushFieldLocks() {
  try {
    await invoke("set_locked_fields", { fields: [...locked] });
  } catch {
    // No library open, so there is no session to lock anything in — and no
    // plan can be built either. The next open pushes the set again.
  }
}

// Read back what the session holds (start-up, and after a window reload). With
// no library open the call fails and the empty set is already the truth.
export async function loadFieldLocks() {
  try {
    const fields = await invoke("locked_fields");
    locked.clear();
    for (const key of fields) locked.add(key);
  } catch {
    locked.clear();
  }
}

// Toggle one or more keys as a unit, push, and repaint what shows a lock.
// Several keys because a row can hold one idea in more than one field — "Track
// 3 / 12" is a number and a total, and locking half of that protects nothing.
export async function toggleFieldLock(keys, onChanged) {
  const turningOn = !keys.every((key) => locked.has(key));
  for (const key of keys) {
    if (turningOn) locked.add(key);
    else locked.delete(key);
  }
  await pushFieldLocks();
  // The table paints locked columns differently and refuses to edit them, so
  // both the header (which carries the padlock) and the rows are rebuilt;
  // whatever asked for the toggle refreshes its own surface.
  hooks.renderTableHead();
  hooks.renderTracks();
  if (onChanged) onChanged();
}

// The padlock button for a field row. `keys` is what it locks, `label` names the
// field in the tooltip, and `onChanged` re-renders the surface it sits on.
export function lockButton(keys, label, onChanged) {
  const on = keys.every((key) => locked.has(key));
  const button = document.createElement("button");
  button.type = "button";
  button.className = "icon fe-lock" + (on ? " on" : "");
  button.innerHTML = ico(on ? "lock" : "unlock");
  button.title = on
    ? `${label} is locked — no operation will change it. Click to unlock.`
    : `Lock ${label} against changes`;
  button.setAttribute("aria-label", button.title);
  button.setAttribute("aria-pressed", String(on));
  button.addEventListener("click", (e) => {
    e.preventDefault();
    e.stopPropagation();
    toggleFieldLock(keys, onChanged);
  });
  return button;
}
