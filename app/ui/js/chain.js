// The transformation rule chain and the action groups behind it (#144 lifted
// this out of generator.js, where it was wired to that panel's element ids).
//
// A chain is an ordered list of cleanup steps over strings. It is a component
// rather than part of GENERATOR because what it acts on is not GENERATOR's
// business: the same chain runs over the files, and over a staged plan when
// there is one (#142). FROM NAME briefly held a second instance (#144); #159
// removed it, since a chain over the plan does that job one step later.
//
// What a panel supplies is its element ids and how the chain is run; what it
// gets back is an object it renders and reads. Nothing here knows about a
// preview, a plan or a panel.
import { el, ico, toast } from "./dom.js";
import { t, tn } from "./i18n.js";
import { invoke } from "./invoke.js";
import { enablePointerReorder } from "./reorder.js";
import { EXTENDED_FIELDS } from "./fields.js";
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

// Every preset checklist built by createPresetChecklist, so a change to the
// library — a preset saved, renamed or deleted in the editor — redraws them all.
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
    // What this step acts on, when it acts on something other than what the
    // chain says (#250). Absent means "whatever the chain says", which is every
    // rule saved before this existed.
    scope: r.scope || null,
  };
}

// Readable names for the scopes whose stored key isn't already a field label.
const SCOPE_LABELS = {
  tags: "all tags",
  filename: "file name",
  fileext: "file extension",
};

// Every target a rule can name (#250): all the tag fields the app models, plus
// the two parts of the file name. The backend matches a scope against a field's
// storage key, so this reads the app's own field table rather than keeping a
// second copy of it in step by hand.
function scopeOptions() {
  return [
    ["tags", "All tag fields"],
    ...EXTENDED_FIELDS,
    ["filename", "File name"],
    ["fileext", "File extension"],
  ];
}

// One-line summary of a group for its tooltip.
function groupSummary(group) {
  const on = (group.rules || []).filter((r) => r.enabled !== false).length;
  const total = (group.rules || []).length;
  const scope = SCOPE_LABELS[group.scope] || group.scope || "all tags";
  return `${on}/${tn("unit.step", total)} · ${scope}`;
}

// ---- the rule chain (#34) ----
//
// `ids` names the elements this instance owns.
function createRuleChain({ ids }) {
  // The rules live for as long as the panel is open; naming and saving chains
  // is the group library's job.
  let rules = [];
  // Stable per-rule id, so pointer-based reorder (#88) can key on identity
  // rather than a shifting array index.
  let ruleIdCounter = 0;

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
      // Where the last rule aims (#251). A chain is usually a run of steps over
      // one field, so inheriting costs one choice for the whole run — and when
      // it is wrong it is wrong in the row you are already looking at.
      scope: rules.length ? rules[rules.length - 1].scope || "tags" : "tags",
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
    // Nothing to clear, nothing to offer (#257).
    if (ids.clear && el(ids.clear)) el(ids.clear).hidden = rules.length === 0;

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
      grip.title = t("action.dragToReorder");
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

      // What this step acts on. Per rule, because one cleanup routinely wants two
      // targets — a catalogue number upper-cased while the titles go to title
      // case — and a chain-wide scope makes that two chains to run in order.
      const scope = document.createElement("select");
      scope.className = "rule-scope";
      scope.title = t("chain.scopeTitle");
      for (const [value, label] of scopeOptions()) {
        const opt = document.createElement("option");
        opt.value = value;
        opt.textContent = label;
        scope.appendChild(opt);
      }
      scope.value = rule.scope || "tags";
      // A group may name something this picker doesn't list — a field added
      // later, or one this build doesn't model. Keep it rather than silently
      // re-aiming the rule at every tag.
      if (scope.value !== (rule.scope || "tags")) {
        const kept = document.createElement("option");
        kept.value = rule.scope;
        kept.textContent = rule.scope;
        scope.appendChild(kept);
        scope.value = rule.scope;
      }
      scope.addEventListener("change", () => {
        rule.scope = scope.value;
      });

      head.append(grip, n, kind, spacer, scope, acts);
      card.append(head);

      // ---- body (per-kind); diacritics is header-only ----
      if (rule.kind === "replace") {
        const b = document.createElement("div");
        b.className = "rule-body";
        const fields = document.createElement("div");
        fields.className = "rule-fields";
        const from = document.createElement("input");
        from.type = "text";
        from.placeholder = t("chain.find");
        from.value = rule.from;
        from.spellcheck = false;
        from.addEventListener("input", () => (rule.from = from.value));
        const to = document.createElement("input");
        to.type = "text";
        to.placeholder = t("chain.replaceWith");
        to.value = rule.to;
        to.spellcheck = false;
        to.addEventListener("input", () => (rule.to = to.value));
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
          box.addEventListener("change", () => (rule[key] = box.checked));
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
          });
          seg.appendChild(btn);
        }
        const note = document.createElement("span");
        note.className = "rule-note";
        note.textContent = t("chain.caseNote");
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
          });
          seg.appendChild(btn);
        }
        const note = document.createElement("span");
        note.className = "rule-note";
        note.textContent = t("chain.keyNote");
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
          "Latin → Russian Cyrillic, for tags that arrived romanized. The whole value converts or none of it does: it needs a trace of romanization (zh, kh, ts, ch, sh, yu, ya…) and nothing in it that was never Cyrillic (q, w, x, a bare c/h/j). So English text is left alone, and so are values mixing the two languages (Zhuk remix). ъ/ь can't be recovered and й/ы both come back as й.";
        b.append(note);
        card.append(b);
      }

      body.append(card);
    });
  }

  // Empty the chain in one act. Offered only when there is something to empty,
  // which is why the button's visibility is the renderer's job (#257).
  function clearRules() {
    if (!rules.length) return;
    rules = [];
    render();
  }

  const chain = {
    ids,
    render,
    addRule,
    clearRules,
    get length() {
      return rules.length;
    },
    // What the chain aims at, taken from its rules: they each carry a target
    // now, so there is no chain-wide one to read (#251). Used to decide whether
    // a run produced renames or tag edits.
    getScopes: () => [...new Set(rules.map((r) => r.scope || "tags"))],
    // The chain as the rules the backend takes.
    rules: () => rules,
    // The chain as a one-off action group — how both the saved-group runner and
    // FROM NAME's cleanup want it.
    // Every rule carries its own target, so the group-level scope is only the
    // fallback for a rule that names none — which, out of this chain, never
    // happens. It stays in the shape because that is what the backend and every
    // saved group expect.
    asGroup: (name = "") => ({ name, scope: "tags", rules: rules.map(ruleForGroup) }),
    // Load a group's steps + scope into the live chain (fresh ids for reorder).
    // Loading a group MATERIALIZES each rule's target (#251): a group written
    // before per-rule scopes carries one scope for all of them, and the chain
    // has nowhere to keep that — so each rule takes it as its own, which is
    // exactly what the backend would have done with it.
    load(group) {
      rules = (group.rules || []).map((r) => ({
        id: ++ruleIdCounter,
        ...ruleForGroup(r),
        scope: r.scope || group.scope || "tags",
      }));
      render();
    },
  };

  el(ids.add).addEventListener("click", addRule);
  if (ids.clear && el(ids.clear)) el(ids.clear).addEventListener("click", clearRules);
  return chain;
}

// ---- the preset checklist (#392) ----
//
// Every preset, yours first and then the built-ins, each with a tick. A tick
// means "run this as part of the job", everywhere at once: the set is shared,
// and each job takes only the rules aimed at what it produces (chains.js). So
// the list also says, per row, whether the preset has anything for the job it
// is shown in — one that only acts on file names reads dimmed under an import.
//
// Presets are not edited here; Edit presets… opens the window where they are
// (#391). The callbacks keep this component free of the set's storage.
function createPresetChecklist({ menu, context, isActive, setActive, appliesTo }) {
  // What a preset acts on, from its rules: tags, the file name, or both.
  function targetLabel(group) {
    const scopes = (group.rules || []).map((r) => r.scope || group.scope || "tags");
    const file = scopes.some((s) => s === "filename" || s === "fileext");
    const tags = scopes.some((s) => s !== "filename" && s !== "fileext");
    if (file && tags) return t("presets.target.both");
    return file ? t("presets.target.file") : t("presets.target.tags");
  }

  function row(group) {
    const label = document.createElement("label");
    label.className = "col-menu-row preset-row preset-check";
    const applies = appliesTo(group, context());
    label.classList.toggle("off", !applies);

    const tick = document.createElement("input");
    tick.type = "checkbox";
    tick.className = "group-tick";
    tick.checked = isActive(group);
    tick.addEventListener("change", () => setActive(group, tick.checked));

    const name = document.createElement("span");
    name.className = "preset-apply";
    const scope = document.createElement("span");
    scope.className = "group-scope";
    scope.textContent = targetLabel(group);
    name.append(document.createTextNode(group.name), scope);

    const summary = group.note ? `${group.note}\n${groupSummary(group)}` : groupSummary(group);
    label.title = applies ? summary : `${summary}\n${t("presets.notHere")}`;
    label.append(tick, name);
    return label;
  }

  function render() {
    const box = el(menu);
    box.innerHTML = "";
    const head = (key) => {
      const h = document.createElement("div");
      h.className = "col-menu-sep";
      h.textContent = t(key);
      box.appendChild(h);
    };
    head("presets.yours");
    if (!actionGroups.length) {
      const empty = document.createElement("div");
      empty.className = "col-menu-sep preset-none";
      empty.textContent = t("presets.noneHere");
      box.appendChild(empty);
    }
    for (const group of actionGroups) box.appendChild(row(group));
    if (builtinGroups.length) {
      head("chain.builtIn");
      for (const group of builtinGroups) box.appendChild(row(group));
    }
  }

  render();
  const api = { render };
  groupMenus.push(api);
  return api;
}

// A preset name nobody has taken: `base`, then `base 2`, `base 3`… `extra` is
// presets about to be added that the library doesn't hold yet.
function uniquePresetName(base, extra = []) {
  const taken = new Set([...actionGroups, ...builtinGroups, ...extra].map((g) => g.name));
  if (!taken.has(base)) return base;
  let n = 2;
  while (taken.has(`${base} ${n}`)) n += 1;
  return `${base} ${n}`;
}

export {
  createPresetChecklist,
  createRuleChain,
  initActionGroups,
  initBuiltinGroups,
  persistActionGroups,
  renderAllGroupsMenus,
  ruleForGroup,
  uniquePresetName,
};
