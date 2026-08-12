// The transformation rule chain and the action groups behind it (#144 lifted
// this out of generator.js, where it was wired to that panel's element ids).
//
// A chain is an ordered list of cleanup steps over strings, and it is the same
// idea wherever values come from: tags already on disk (GENERATOR) or values a
// mask has just pulled out of a file name (TAGGER › FROM NAME). So the cards,
// the Groups popover and the saved `action_groups` are one component with two
// instances rather than two mechanisms in neighbouring panels.
//
// What a panel supplies is its element ids and how the chain is run; what it
// gets back is an object it renders and reads. Nothing here knows about a
// preview, a plan or a panel.
import { el, ico, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { enablePointerReorder } from "./reorder.js";
import {
  actionGroups,
  builtinGroups,
  savedSettings,
  setActionGroups,
  setBuiltinGroups,
  setSavedSettings,
} from "./state.js";

// ---- the shared group library (#57, #137) ----
// Saved groups live in settings.json and the shipped presets come from the
// backend; both are global, so every chain instance offers the same shelf.

// Every Groups popover built by createGroupsMenu, so a save or a delete
// refreshes all of them instead of only the one that was open. Ticks are per
// popover — what GENERATOR is about to run has nothing to do with what FROM
// NAME cleans its extracted values with — so a delete has to reach all of them.
const groupMenus = [];

function renderAllGroupsMenus() {
  for (const menu of groupMenus) menu.render();
}

async function initActionGroups() {
  try {
    setSavedSettings((await invoke("load_settings", {})) || {});
    setActionGroups(Array.isArray(savedSettings.action_groups) ? savedSettings.action_groups : []);
  } catch (e) {
    setActionGroups([]);
  }
  renderAllGroupsMenus();
}

async function persistActionGroups() {
  setSavedSettings({ ...savedSettings, action_groups: actionGroups });
  try {
    await invoke("save_settings", { settings: savedSettings });
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- the shipped preset library (#137) ----
// Action groups that come with the app rather than being saved by the user.
// They are ordinary groups in every way that matters — same rule shape, same
// scope, run and loaded through the same code — but they live in the binary,
// not in settings.json, so they can't be deleted and can't drift. Loading one
// copies its steps into the live chain, where they can be edited and saved
// under a new name; the preset itself stays as shipped.
//
// The list is the backend's (`builtin_action_groups`) rather than a copy here,
// so a preset's patterns are covered by the tests that build them into chains.
async function initBuiltinGroups() {
  try {
    setBuiltinGroups((await invoke("builtin_action_groups")).map((g) => ({ ...g, builtin: true })));
  } catch (e) {
    setBuiltinGroups([]); // no shelf is better than a broken one
  }
  renderAllGroupsMenus();
}

// A plain, serializable copy of one transform rule (no DOM id, `enabled` normalized).
function ruleForGroup(r) {
  return {
    kind: r.kind,
    from: r.from || "",
    to: r.to || "",
    regex: !!r.regex,
    whole_word: !!r.whole_word,
    case_sensitive: !!r.case_sensitive,
    style: r.style || "",
    enabled: r.enabled !== false,
  };
}

// Readable names for the scopes whose stored key isn't already a field label.
const SCOPE_LABELS = {
  tags: "all tags",
  filename: "file name",
  fileext: "file extension",
};

// The scopes that only make sense against a file on disk. FROM NAME hides
// groups carrying one: there is no file name or extension among the values a
// mask extracts, so such a group would silently do nothing there.
const FILE_SCOPES = ["filename", "fileext"];

// One-line summary of a group for its tooltip.
function groupSummary(group) {
  const on = (group.rules || []).filter((r) => r.enabled !== false).length;
  const total = (group.rules || []).length;
  const scope = SCOPE_LABELS[group.scope] || group.scope || "all tags";
  return `${on}/${total} step(s) · ${scope}`;
}

// ---- the rule chain (#34) ----
//
// `ids` names the elements this instance owns; `onChange` is called whenever
// the chain or its scope changes, which is how a panel with a live read-out
// (FROM NAME) keeps up without polling.
function createRuleChain({ ids, onChange = () => {} }) {
  // The rules live for as long as the panel is open; naming and saving chains
  // is the group library's job.
  let rules = [];
  // Stable per-rule id, so pointer-based reorder (#88) can key on identity
  // rather than a shifting array index.
  let ruleIdCounter = 0;

  const changed = () => onChange();

  function addRule() {
    const kind = el(ids.kind).value;
    rules.push({
      id: ++ruleIdCounter,
      kind,
      from: "",
      to: "",
      regex: false,
      whole_word: false,
      case_sensitive: false,
      style: kind === "case" ? "title" : kind === "key" ? "camelot" : "",
      enabled: true,
    });
    render();
  }

  function moveRule(from, to) {
    if (to < 0 || to >= rules.length) return;
    [rules[from], rules[to]] = [rules[to], rules[from]];
    render();
  }

  function mkRuleIcon(iconName, title, disabled, onClick) {
    const b = document.createElement("button");
    b.className = "icon";
    b.innerHTML = ico(iconName);
    b.title = title;
    b.setAttribute("aria-label", title);
    b.disabled = disabled;
    b.addEventListener("click", onClick);
    return b;
  }

  function render() {
    const body = el(ids.rules);
    body.innerHTML = "";
    el(ids.empty).hidden = rules.length > 0;

    rules.forEach((rule, index) => {
      const card = document.createElement("div");
      card.className = "rule-card";
      // A disabled step (#57) stays in the chain but is skipped and shown dimmed.
      card.classList.toggle("rule-disabled", rule.enabled === false);
      card.dataset.index = index;
      card.dataset.key = rule.id; // identity key for pointer reorder (#88)

      // ---- header: grip · n · kind · ↑ ↓ ✕ ----
      const head = document.createElement("div");
      head.className = "rule-head";

      const grip = document.createElement("span");
      grip.className = "rule-grip";
      grip.innerHTML = ico("grip");
      grip.title = "Drag to reorder";
      // Order is semantic (case before/after an acronym fix differs). Pointer-based
      // reorder — WKWebView's HTML5 DnD is unreliable (#88); ↑/↓ stay as fallback.
      enablePointerReorder(grip, card, el(ids.rules), ".rule-card", (draggedKey, targetKey, below) => {
        const dragged = rules.find((r) => String(r.id) === draggedKey);
        if (!dragged) return;
        const order = rules.filter((r) => r !== dragged);
        let to = order.findIndex((r) => String(r.id) === targetKey);
        if (to < 0) return;
        if (below) to += 1;
        order.splice(to, 0, dragged);
        rules = order;
        render();
      });

      const n = document.createElement("span");
      n.className = "rule-n";
      n.textContent = index + 1;

      const kind = document.createElement("span");
      kind.className = "rule-kind";
      kind.textContent =
        rule.kind === "replace"
          ? "Find and replace"
          : rule.kind === "case"
            ? "Change case"
            : rule.kind === "key"
              ? "Key notation"
              : rule.kind === "transliterate"
                ? "Transliterate to Latin"
                : rule.kind === "untransliterate"
                  ? "Transliterate to Cyrillic"
                  : "Remove diacritics";

      const spacer = document.createElement("span");
      spacer.className = "spacer";

      const acts = document.createElement("span");
      acts.className = "rule-acts";
      // Enable/disable this step (#57): kept in the chain either way, skipped when off.
      const toggle = mkRuleIcon(
        "check",
        rule.enabled === false ? "Step off — click to enable" : "Step on — click to disable",
        false,
        () => {
          rule.enabled = rule.enabled === false;
          render();
        }
      );
      toggle.classList.add("rule-toggle");
      if (rule.enabled === false) toggle.classList.add("off");
      acts.append(toggle);
      // ↑/↓ stay as the keyboard / no-pointer fallback for reordering.
      acts.append(
        mkRuleIcon("caret-up", "Move up", index === 0, () => moveRule(index, index - 1)),
        mkRuleIcon("caret-down", "Move down", index === rules.length - 1, () =>
          moveRule(index, index + 1)
        )
      );
      const remove = mkRuleIcon("close", "Remove rule", false, () => {
        rules.splice(index, 1);
        render();
      });
      remove.classList.add("rm");
      acts.append(remove);

      head.append(grip, n, kind, spacer, acts);
      card.append(head);

      // ---- body (per-kind); diacritics is header-only ----
      if (rule.kind === "replace") {
        const b = document.createElement("div");
        b.className = "rule-body";
        const fields = document.createElement("div");
        fields.className = "rule-fields";
        const from = document.createElement("input");
        from.type = "text";
        from.placeholder = "find";
        from.value = rule.from;
        from.spellcheck = false;
        from.addEventListener("input", () => {
          rule.from = from.value;
          changed();
        });
        const to = document.createElement("input");
        to.type = "text";
        to.placeholder = "replace with";
        to.value = rule.to;
        to.spellcheck = false;
        to.addEventListener("input", () => {
          rule.to = to.value;
          changed();
        });
        fields.append(from, to);

        const flags = document.createElement("div");
        flags.className = "rule-flags";
        for (const [key, text, hint] of [
          ["regex", "regex", "Treat the pattern as a regular expression"],
          ["whole_word", "whole word", "Only match complete words"],
          ["case_sensitive", "match case", "Distinguish upper and lower case"],
        ]) {
          const label = document.createElement("label");
          label.className = "rule-flag";
          label.title = hint;
          const box = document.createElement("input");
          box.type = "checkbox";
          box.checked = rule[key];
          box.addEventListener("change", () => {
            rule[key] = box.checked;
            changed();
          });
          label.append(box, document.createTextNode(text));
          flags.appendChild(label);
        }
        b.append(fields, flags);
        card.append(b);
      } else if (rule.kind === "case") {
        const b = document.createElement("div");
        b.className = "rule-body";
        const seg = document.createElement("div");
        seg.className = "seg";
        for (const [value, text] of [
          ["title", "Title"],
          ["lower", "lower"],
          ["upper", "UPPER"],
          ["sentence", "Sentence"],
        ]) {
          const btn = document.createElement("button");
          btn.type = "button";
          btn.className = "seg-btn" + (rule.style === value ? " active" : "");
          btn.textContent = text;
          btn.addEventListener("click", () => {
            rule.style = value;
            seg.querySelectorAll(".seg-btn").forEach((s) => s.classList.toggle("active", s === btn));
            changed();
          });
          seg.appendChild(btn);
        }
        const note = document.createElement("span");
        note.className = "rule-note";
        note.textContent = "Known acronyms & roman numerals keep their casing.";
        b.append(seg, note);
        card.append(b);
      } else if (rule.kind === "key") {
        const b = document.createElement("div");
        b.className = "rule-body";
        const seg = document.createElement("div");
        seg.className = "seg";
        for (const [value, text] of [
          ["camelot", "Camelot"],
          ["openkey", "Open Key"],
          ["musical", "Musical"],
        ]) {
          const btn = document.createElement("button");
          btn.type = "button";
          btn.className = "seg-btn" + (rule.style === value ? " active" : "");
          btn.textContent = text;
          btn.addEventListener("click", () => {
            rule.style = value;
            seg.querySelectorAll(".seg-btn").forEach((s) => s.classList.toggle("active", s === btn));
            changed();
          });
          seg.appendChild(btn);
        }
        const note = document.createElement("span");
        note.className = "rule-note";
        note.textContent = "Converts the musical key (best scoped to the Key field). Unrecognized values are left as-is.";
        b.append(seg, note);
        card.append(b);
      } else if (rule.kind === "untransliterate") {
        // The one step whose losses are worth stating on the card: reversing a
        // romanization can't recover what the forward direction dropped, and the
        // per-word guard is the reason English text survives it.
        const b = document.createElement("div");
        b.className = "rule-body";
        const note = document.createElement("span");
        note.className = "rule-note";
        note.textContent =
          "Latin → Russian Cyrillic, for tags that arrived romanized. A word with no Cyrillic reading (Jazz, The) is left alone; ъ/ь can't be recovered and й/ы both come back as й.";
        b.append(note);
        card.append(b);
      }

      body.append(card);
    });
    changed();
  }

  const chain = {
    ids,
    render,
    addRule,
    get length() {
      return rules.length;
    },
    getScope: () => el(ids.scope).value,
    setScope(value) {
      if (value) el(ids.scope).value = value;
    },
    // The chain as the rules the backend takes.
    rules: () => rules,
    // The chain as a one-off action group — how both the saved-group runner and
    // FROM NAME's cleanup want it.
    asGroup: (name = "") => ({ name, scope: el(ids.scope).value, rules: rules.map(ruleForGroup) }),
    // Load a group's steps + scope into the live chain (fresh ids for reorder).
    load(group) {
      rules = (group.rules || []).map((r) => ({ id: ++ruleIdCounter, ...ruleForGroup(r) }));
      chain.setScope(group.scope);
      render();
    },
  };

  el(ids.add).addEventListener("click", addRule);
  // The scope is part of the chain, so a panel watching the chain has to hear it.
  el(ids.scope).addEventListener("change", changed);
  return chain;
}

// ---- the Groups popover (#57/#137) ----
//
// A checklist of saved and shipped groups over one chain. Ticking rather than
// running on click is what lets several groups take part at once: per-field
// cleanup is two or three of these together, and one at a time means previewing
// and applying each in turn.
//
// What the ticks feed differs by panel. GENERATOR runs them as one plan, so it
// passes `onRun` and gets a Run button; FROM NAME folds them into its own
// Preview instead, so it watches `onTicksChanged` and there is no button.
// Ticks are session-only and private to this popover: a tick says "these, now".
function createGroupsMenu({
  btn,
  menu,
  chain,
  hideFileScopes = false,
  tickTitle = "Include in the next run",
  onRun,
  onTicksChanged = () => {},
}) {
  const tickedGroups = new Set();
  const visible = (list) =>
    hideFileScopes ? list.filter((g) => !FILE_SCOPES.includes(g.scope)) : list;

  // Every group the checklist can offer, saved ones first — the order they run in.
  const allGroups = () => [...visible(actionGroups), ...visible(builtinGroups)];
  const tickedInOrder = () => allGroups().filter((g) => tickedGroups.has(g.name));

  function updateRunTicked() {
    const run = el(menu).querySelector(".preset-run > button");
    if (run) {
      const n = tickedInOrder().length;
      run.disabled = n === 0;
      run.textContent = n ? `Run ${n} ticked` : "Run ticked";
    }
    onTicksChanged(tickedInOrder());
  }

  function toggleTicked(name, on) {
    if (on) tickedGroups.add(name);
    else tickedGroups.delete(name);
    updateRunTicked();
  }

  // Save the current chain (+ scope) under `name`, replacing a same-named group.
  function saveCurrentGroup(name) {
    name = name.trim();
    if (!name) return;
    if (chain.length === 0) {
      toast("Add at least one rule before saving a group", true);
      return;
    }
    setActionGroups(actionGroups.filter((g) => g.name !== name));
    actionGroups.push(chain.asGroup(name));
    actionGroups.sort((a, b) => a.name.localeCompare(b.name));
    persistActionGroups();
    renderAllGroupsMenus();
    toast(`Saved action group “${name}”`);
  }

  function deleteGroup(name) {
    setActionGroups(actionGroups.filter((g) => g.name !== name));
    // A tick on a group that no longer exists would contribute nothing, in this
    // popover or any other.
    for (const other of groupMenus) other.dropTick(name);
    persistActionGroups();
    renderAllGroupsMenus();
  }

  // One checklist row (#137): a tick, the name with the scope it acts on, and
  // Load. Built-ins get no Delete — they aren't the user's to remove — and carry
  // their note in the tooltip instead of the bare summary.
  function groupMenuRow(group) {
    const row = document.createElement("div");
    row.className = "col-menu-row preset-row";

    const tick = document.createElement("input");
    tick.type = "checkbox";
    tick.className = "group-tick";
    tick.checked = tickedGroups.has(group.name);
    tick.title = tickTitle;
    tick.addEventListener("change", (e) => {
      e.stopPropagation();
      toggleTicked(group.name, tick.checked);
    });

    const name = document.createElement("button");
    name.type = "button";
    name.className = "text-btn preset-apply";
    const scope = document.createElement("span");
    scope.className = "group-scope";
    scope.textContent = SCOPE_LABELS[group.scope] || group.scope || "all tags";
    name.append(document.createTextNode(group.name), scope);
    name.title = group.note ? `${group.note}\n${groupSummary(group)}` : groupSummary(group);
    // The name is the checkbox's label, just a bigger target for it.
    name.addEventListener("click", () => {
      tick.checked = !tick.checked;
      toggleTicked(group.name, tick.checked);
    });

    const load = document.createElement("button");
    load.type = "button";
    load.className = "text-btn group-load";
    load.textContent = "Load";
    load.title = group.builtin
      ? "Load into the chain to edit — the built-in stays as shipped"
      : "Load into the chain without running";
    load.addEventListener("click", (e) => {
      e.stopPropagation();
      chain.load(group);
      el(menu).hidden = true;
    });

    row.append(tick, name, load);

    if (!group.builtin) {
      const del = document.createElement("button");
      del.type = "button";
      del.className = "preset-del";
      del.innerHTML = ico("close");
      del.title = `Delete “${group.name}”`;
      del.addEventListener("click", (e) => {
        e.stopPropagation();
        deleteGroup(group.name);
      });
      row.append(del);
    }

    return row;
  }

  // Build the popover — mirrors the presets menu (#44): a row per group, plus a
  // footer to name and save the current chain. The user's own groups come
  // first: they're the ones being iterated on, and the shipped library below
  // them (#137) is a stable shelf to reach for.
  function render() {
    const box = el(menu);
    box.innerHTML = "";
    const saved = visible(actionGroups);
    const shipped = visible(builtinGroups);
    if (!saved.length) {
      const empty = document.createElement("div");
      empty.className = "col-menu-sep";
      empty.textContent = "No saved groups";
      box.appendChild(empty);
    }
    for (const group of saved) box.appendChild(groupMenuRow(group));

    if (shipped.length) {
      const head = document.createElement("div");
      head.className = "col-menu-sep";
      head.textContent = "Built-in";
      box.appendChild(head);
      for (const group of shipped) box.appendChild(groupMenuRow(group));
    }

    // The checklist's one action, above the save row: what the ticks are for.
    // Only where the ticks run on their own — FROM NAME folds them into its own
    // Preview, so a second button there would be a second way to do it.
    if (onRun) {
      const runFoot = document.createElement("div");
      runFoot.className = "col-menu-foot preset-run";
      const runBtn = document.createElement("button");
      runBtn.type = "button";
      runBtn.className = "text-btn";
      runBtn.title = "Preview the ticked groups, run in list order, as one plan";
      runBtn.addEventListener("click", () => {
        box.hidden = true;
        onRun(tickedInOrder());
      });
      runFoot.appendChild(runBtn);
      box.appendChild(runFoot);
    }

    const foot = document.createElement("div");
    foot.className = "col-menu-foot preset-save";
    const input = document.createElement("input");
    input.type = "text";
    input.placeholder = "Save current chain as…";
    input.spellcheck = false;
    input.className = "preset-name";
    const save = document.createElement("button");
    save.type = "button";
    save.className = "text-btn";
    save.textContent = "Save";
    const commit = () => {
      if (input.value.trim()) {
        saveCurrentGroup(input.value);
        input.value = "";
      }
    };
    save.addEventListener("click", commit);
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        commit();
      }
    });
    foot.append(input, save);
    box.appendChild(foot);
    updateRunTicked();
  }

  // Toggle + outside-click close, the same as the presets menu.
  el(btn).addEventListener("click", (e) => {
    e.stopPropagation();
    const box = el(menu);
    if (box.hidden) render();
    box.hidden = !box.hidden;
  });
  document.addEventListener("click", (e) => {
    const box = el(menu);
    if (!box.hidden && !box.contains(e.target) && !el(btn).contains(e.target)) {
      box.hidden = true;
    }
  });

  const api = {
    render,
    tickedInOrder,
    dropTick(name) {
      if (tickedGroups.delete(name)) updateRunTicked();
    },
  };
  groupMenus.push(api);
  return api;
}

export {
  createGroupsMenu,
  createRuleChain,
  initActionGroups,
  initBuiltinGroups,
  renderAllGroupsMenus,
  ruleForGroup,
};
