// The app's own option list for every <select> (#845).
//
// The native element stays exactly where it is, styled and sized as before,
// and stays the one source of truth: code reads and sets `.value` as it always
// has. Only the opening is taken over. Instead of the platform's menu (a
// different look on each platform, no animation, no room for a line of
// explanation) a list of our own opens under the select, ticks the chosen
// option, can show an option's `data-description`, and on a pick sets the
// value and fires `input` and `change` the way the platform does.
//
// It works by delegation, so selects built at run time are covered too. A
// select with `multiple`, a `size` above 1, or `data-native-select` keeps the
// platform's behaviour.
import { escapeHtml, placeFloating } from "./dom.js";

const TYPEAHEAD_MS = 600;

let list = null; // the open list, or null
let owner = null; // the select it belongs to
let options = []; // [{ option, index }] in list order, selectable or not
let active = -1; // position in `options`
let typed = "";
let typedAt = 0;

function takesOver(select) {
  return (
    select instanceof HTMLSelectElement &&
    !select.multiple &&
    !(select.size > 1) &&
    !select.disabled &&
    !("nativeSelect" in select.dataset)
  );
}

function selectable(entry) {
  return entry && !entry.option.disabled && !entry.option.parentElement?.disabled;
}

function labelOf(option) {
  return option.label || option.textContent || option.value;
}

function render(select) {
  options = [];
  const parts = [];
  let group = null;
  for (const option of select.options) {
    if (option.hidden) continue;
    const parent = option.parentElement;
    if (parent instanceof HTMLOptGroupElement && parent !== group) {
      group = parent;
      parts.push(`<div class="sel-group" role="presentation">${escapeHtml(parent.label)}</div>`);
    }
    const at = options.length;
    options.push({ option, index: option.index });
    const chosen = option.index === select.selectedIndex;
    const off = !selectable(options[at]);
    const description = option.dataset.description;
    parts.push(
      `<div class="sel-option${chosen ? " chosen" : ""}${off ? " off" : ""}" role="option" id="sel-opt-${at}" data-at="${at}"` +
        ` aria-selected="${chosen}"${off ? ' aria-disabled="true"' : ""}>` +
        `<svg class="ico sel-tick" aria-hidden="true"><use href="#i-check"/></svg>` +
        `<span class="sel-text"><span class="sel-label">${escapeHtml(labelOf(option))}</span>` +
        (description ? `<span class="sel-desc">${escapeHtml(description)}</span>` : "") +
        `</span></div>`,
    );
  }
  list.innerHTML = parts.join("");
}

function setActive(at, { scroll = true } = {}) {
  const previous = list.querySelector(".sel-option.active");
  if (previous) previous.classList.remove("active");
  active = at;
  const row = list.querySelector(`[data-at="${at}"]`);
  if (!row) {
    owner.removeAttribute("aria-activedescendant");
    return;
  }
  row.classList.add("active");
  owner.setAttribute("aria-activedescendant", row.id);
  if (scroll) row.scrollIntoView({ block: "nearest" });
}

function step(from, delta) {
  for (let at = from + delta; at >= 0 && at < options.length; at += delta) {
    if (selectable(options[at])) return at;
  }
  return from;
}

function open(select) {
  close();
  owner = select;
  list = document.createElement("div");
  list.className = "sel-list";
  list.setAttribute("role", "listbox");
  const name = select.getAttribute("aria-label") || select.labels?.[0]?.textContent?.trim();
  if (name) list.setAttribute("aria-label", name);
  render(select);
  document.body.appendChild(list);
  const rect = select.getBoundingClientRect();
  list.style.minWidth = `${Math.round(rect.width)}px`;
  placeFloating(list, select, { gap: 4 });
  // Opened above its select, it rises from below instead of dropping.
  if (list.style.top === "auto") list.classList.add("above");
  select.setAttribute("aria-expanded", "true");
  select.setAttribute("aria-controls", "sel-list");
  list.id = "sel-list";
  const current = options.findIndex((entry) => entry.index === select.selectedIndex);
  setActive(current >= 0 ? current : step(-1, 1));
  select.focus({ preventScroll: true });
}

function close() {
  if (!list) return;
  list.remove();
  list = null;
  if (owner) {
    owner.setAttribute("aria-expanded", "false");
    owner.removeAttribute("aria-activedescendant");
  }
  owner = null;
  options = [];
  active = -1;
}

function choose(at) {
  const entry = options[at];
  const select = owner;
  close();
  if (!select) return;
  select.focus({ preventScroll: true });
  if (!selectable(entry) || entry.index === select.selectedIndex) return;
  select.selectedIndex = entry.index;
  select.dispatchEvent(new Event("input", { bubbles: true }));
  select.dispatchEvent(new Event("change", { bubbles: true }));
}

function typeahead(char) {
  const now = Date.now();
  typed = now - typedAt > TYPEAHEAD_MS ? char : typed + char;
  typedAt = now;
  const needle = typed.toLocaleLowerCase();
  // A repeated single letter cycles through the options that start with it.
  const cycling = typed.length > 1 && [...typed].every((c) => c === typed[0]);
  const query = cycling ? typed[0].toLocaleLowerCase() : needle;
  const start = cycling || typed.length === 1 ? active + 1 : active;
  for (let i = 0; i < options.length; i++) {
    const at = (start + i) % options.length;
    if (selectable(options[at]) && labelOf(options[at].option).toLocaleLowerCase().startsWith(query)) {
      setActive(at);
      return;
    }
  }
}

// Captured on the window, so an open list has the keys before any app-wide
// handler (Escape would otherwise also discard a staged plan, #533).
window.addEventListener(
  "keydown",
  (e) => {
    if (list) {
      if (e.target !== owner) return;
      const plain = !e.metaKey && !e.ctrlKey;
      let handled = true;
      if (e.key === "ArrowDown" && plain) setActive(step(active, 1));
      else if (e.key === "ArrowUp" && plain) setActive(step(active, -1));
      else if (e.key === "Home" || e.key === "PageUp") setActive(step(-1, 1));
      else if (e.key === "End" || e.key === "PageDown") setActive(step(options.length, -1));
      else if ((e.key === "Enter" || e.key === " ") && plain && !(e.key === " " && typed && Date.now() - typedAt < TYPEAHEAD_MS))
        choose(active);
      else if (e.key === "Escape") close();
      else if (e.key === "Tab") {
        close();
        handled = false;
      } else if (e.key.length === 1 && plain && !e.altKey) typeahead(e.key);
      else handled = false;
      if (handled) {
        // The app's own window-level handlers sit on this same target, so
        // only stopping the rest of them here keeps them out of it.
        e.preventDefault();
        e.stopImmediatePropagation();
      }
      return;
    }
    const select = e.target;
    if (!takesOver(select)) return;
    const opens =
      ((e.key === "ArrowDown" || e.key === "ArrowUp") && !e.metaKey && !e.ctrlKey) ||
      (e.key === " " && !e.metaKey && !e.ctrlKey && !e.altKey) ||
      e.key === "F4";
    if (!opens) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    open(select);
  },
  true,
);

window.addEventListener(
  "mousedown",
  (e) => {
    if (e.button !== 0) return;
    if (list && list.contains(e.target)) {
      // Keep the focus on the select while a row is clicked.
      e.preventDefault();
      return;
    }
    const select = e.target instanceof Element ? e.target.closest("select") : null;
    if (select && takesOver(select)) {
      e.preventDefault();
      if (owner === select) close();
      else open(select);
      return;
    }
    close();
  },
  true,
);

document.addEventListener("click", (e) => {
  if (!list) return;
  const row = e.target instanceof Element ? e.target.closest(".sel-option") : null;
  if (row && list.contains(row)) choose(Number(row.dataset.at));
});

document.addEventListener("mousemove", (e) => {
  if (!list) return;
  const row = e.target instanceof Element ? e.target.closest(".sel-option") : null;
  if (row && list.contains(row) && !row.classList.contains("off") && Number(row.dataset.at) !== active) {
    setActive(Number(row.dataset.at), { scroll: false });
  }
});

// The list is placed against the select once; anything that moves the select
// closes it rather than leaving it behind.
window.addEventListener(
  "scroll",
  (e) => {
    if (list && !list.contains(e.target)) close();
  },
  true,
);
window.addEventListener("resize", close);
window.addEventListener("blur", close);
document.addEventListener("focusin", (e) => {
  if (list && e.target !== owner && !list.contains(e.target)) close();
});
