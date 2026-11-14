import { describeRule } from "./preset-presentation.js";
import { createComponentElement } from "../lib/component-elements.js";
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
import { t, tn, tOr } from "./i18n.js";
import { invoke } from "./invoke.js";
import { enablePointerReorder } from "./reorder.js";
import { EXTENDED_FIELDS, fieldLabel } from "./fields.js";
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
    // Title case's small words (#537); absent means off.
    small_words: Array.isArray(r.small_words) ? [...r.small_words] : null,
  };
}

// The small-word lists a title-case rule can keep lowercase (#537), from the
// backend so the card offers what the transform applies. Named in their own
// language, like the language setting.
const SMALL_LIST_NAMES = { en: "English", de: "Deutsch", es: "Español", fr: "Français", it: "Italiano" };
let smallWordLists = [];
invoke("small_word_lists")
  .then((lists) => (smallWordLists = lists || []))
  .catch(() => {});

function smallListMatching(words) {
  const key = (list) => [...list].sort().join(" ");
  const found = smallWordLists.find(([, list]) => key(list) === key(words || []));
  return found ? found[0] : "custom";
}

// The title-case option (#537): a tick, then the list it keeps lowercase, picked
// by language and editable as text. Off is no list at all.
function smallWordsControl(rule) {
  const box = document.createElement("div");
  box.className = "rule-small";
  const tick = document.createElement("label");
  tick.className = "rule-flag";
  const check = createComponentElement("input", { type: "checkbox" });
  check.checked = Array.isArray(rule.small_words);
  const text = document.createElement("span");
  text.textContent = t("chain.smallWords");
  tick.title = t("chain.smallWordsTitle");
  tick.append(check, text);

  const pick = document.createElement("select");
  pick.className = "rule-small-lang";
  for (const [lang] of smallWordLists) {
    const option = document.createElement("option");
    option.value = lang;
    option.textContent = SMALL_LIST_NAMES[lang] || lang;
    pick.appendChild(option);
  }
  const custom = document.createElement("option");
  custom.value = "custom";
  custom.textContent = t("chain.smallCustom");
  pick.appendChild(custom);

  const field = document.createElement("div");
  field.className = "field sm rule-small-words";
  const words = createComponentElement("input");
  words.type = "text";
  words.spellcheck = false;
  words.setAttribute("aria-label", t("chain.smallWords"));
  field.appendChild(words);

  const paint = () => {
    const on = Array.isArray(rule.small_words);
    pick.hidden = field.hidden = !on;
    if (!on) return;
    pick.value = smallListMatching(rule.small_words);
    if (document.activeElement !== words) words.value = rule.small_words.join(", ");
  };
  check.addEventListener("change", () => {
    if (check.checked) {
      // The interface language's list when there is one, else English.
      const lang = document.documentElement.lang;
      const list = (smallWordLists.find(([code]) => code === lang) || smallWordLists[0] || ["", []])[1];
      rule.small_words = [...list];
    } else {
      rule.small_words = null;
    }
    paint();
  });
  pick.addEventListener("change", () => {
    const found = smallWordLists.find(([code]) => code === pick.value);
    if (found) rule.small_words = [...found[1]];
    paint();
  });
  words.addEventListener("input", () => {
    rule.small_words = words.value
      .split(/[\s,]+/)
      .map((word) => word.trim().toLowerCase())
      .filter(Boolean);
    pick.value = smallListMatching(rule.small_words);
  });
  box.append(tick, pick, field);
  paint();
  return box;
}

// Readable names for the scopes whose stored key isn't already a field label.
const SCOPE_LABELS = {
  tags: "chain.scope.tags",
  filename: "chain.scope.filename",
  fileext: "chain.scope.fileext",
};

// Every target a rule can name (#250): all the tag fields the app models, plus
// the two parts of the file name. The backend matches a scope against a field's
// storage key, so this reads the app's own field table rather than keeping a
// second copy of it in step by hand.
function scopeOptions() {
  return [
    ["tags", t("chain.scopeAll")],
    ...EXTENDED_FIELDS.map(([key]) => [key, fieldLabel(key)]),
    ["filename", t("chain.scopeFile")],
    ["fileext", t("chain.scopeExt")],
  ];
}

// One-line summary of a group for its tooltip.
function groupSummary(group) {
  const on = (group.rules || []).filter((r) => r.enabled !== false).length;
  const total = (group.rules || []).length;
  const scope = SCOPE_LABELS[group.scope]
    ? t(SCOPE_LABELS[group.scope])
    : group.scope
      ? fieldLabel(group.scope)
      : t("chain.scope.tags");
  return `${on}/${tn("unit.step", total)} · ${scope}`;
}

// A built-in preset's name and note in the current language (#625). The
// English name the backend sends stays the preset's identity, since the
// ticked sets are keyed by it; only what is shown is translated.
function builtinKey(group, part) {
  return `preset.${group.name.toLowerCase().replace(/[^a-z0-9]+/g, "-")}.${part}`;
}

export function presetLabel(group) {
  return group.builtin ? tOr(builtinKey(group, "name"), group.name) : group.name;
}

export function presetNote(group) {
  return group.builtin ? tOr(builtinKey(group, "note"), group.note) : group.note;
}

// ---- the rule chain (#34) ----
//
// `ids` names the elements this instance owns.
// Deferred focus belongs to the action that requested it. Later input cancels
// that request; focus changes made by dialog/control setup do not.
let interactionRevision = 0;
for (const type of ["pointerdown", "keydown", "input"]) {
  document.addEventListener(type, () => interactionRevision++, true);
}

function createRuleChain({ ids }) {
  // The rules live for as long as the panel is open; naming and saving chains
  // is the group library's job.
  let rules = [];
  const inline = ids.rules === "pe-rules";
  let expandedRule = null;
  // Stable per-rule id, so pointer-based reorder (#88) can key on identity
  // rather than a shifting array index.
  let ruleIdCounter = 0;

  function addRule(kind = el(ids.kind).value) {
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
    if (inline) expandedRule = ruleIdCounter;
    render();
    if (inline) {
      const revisionAtAdd = interactionRevision;
      requestAnimationFrame(() => requestAnimationFrame(() => {
        // Enhancement and menu teardown finish before focusing a new field.
        // Never take focus back if the person has already moved elsewhere.
        if (interactionRevision !== revisionAtAdd) return;
        const editor = el(ids.rules).querySelector(`[data-key="${expandedRule}"] .pe-inline-editor`);
        (editor?.querySelector("input[type=text]") || editor?.querySelector("select"))?.focus();
      }));
    }
  }

  function moveRule(from, to) {
    if (to < 0 || to >= rules.length) return;
    [rules[from], rules[to]] = [rules[to], rules[from]];
    render();
  }

  function mkRuleIcon(iconName, title, disabled, onClick) {
    const b = createComponentElement("button");
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
    const active = document.activeElement;
    const revisionAtRender = interactionRevision;
    const readOnly = inline && el(ids.set)?.dataset.readonly === "true";
    const focusKey = active?.closest(".rule-card")?.dataset.key;
    const focusLabel = active?.getAttribute("aria-label");
    body.innerHTML = "";
    el(ids.empty).hidden = rules.length > 0;
    // Nothing to clear, nothing to offer (#257).
    if (ids.clear && el(ids.clear)) el(ids.clear).hidden = rules.length === 0;

    rules.forEach((rule, index) => {
      const card = document.createElement("div");
      card.className = "rule-card";
      card.classList.toggle("pe-builtin-rule", readOnly);
      // A disabled step (#57) stays in the chain but is skipped and shown dimmed.
      card.classList.toggle("rule-disabled", rule.enabled === false);
      card.dataset.index = index;
      card.dataset.key = rule.id; // identity key for pointer reorder (#88)

      // ---- header: grip · on/off · n · kind · target ……… ↑ ↓ | ✕ (#556) ----
      const head = document.createElement("div");
      head.className = "rule-head";

      const grip = document.createElement("span");
      grip.className = "rule-grip";
      grip.innerHTML = ico("grip");
      grip.title = t("action.dragToReorder");
      // Order is semantic (case before/after an acronym fix differs). Pointer-based
      // reorder — WKWebView's HTML5 DnD is unreliable (#88); ↑/↓ stay as fallback.
      if (!readOnly) enablePointerReorder(grip, card, el(ids.rules), ".rule-card", (draggedKey, targetKey, below) => {
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
          ? t("rule.replace")
          : rule.kind === "case"
            ? t("rule.case")
            : rule.kind === "key"
              ? t("rule.key")
              : rule.kind === "transliterate"
                ? t("rule.transliterate")
                : rule.kind === "untransliterate"
                  ? t("rule.untransliterate")
                  : t("rule.diacritics");

      const spacer = document.createElement("span");
      spacer.className = "spacer";

      const acts = document.createElement("span");
      acts.className = "rule-acts";
      // Enable/disable this step (#57): kept in the chain either way, skipped
      // when off. A checkbox (#556): it answers yes or no about the step, where
      // the tick button it replaces read as one more action beside ↑ ↓ ✕.
      const toggle = createComponentElement("input", { type: "checkbox" });
      toggle.className = "rule-toggle";
      toggle.checked = rule.enabled !== false;
      toggle.title = rule.enabled === false ? t("chain.stepOff") : t("chain.stepOn");
      toggle.setAttribute("aria-label", toggle.title);
      toggle.addEventListener("change", () => {
        rule.enabled = toggle.checked;
        render();
      });
      // ↑/↓ stay as the keyboard / no-pointer fallback for reordering; a
      // hairline keeps ✕ apart from them.
      const sep = document.createElement("span");
      sep.className = "field-sep";
      sep.setAttribute("aria-hidden", "true");
      acts.append(
        mkRuleIcon("caret-up", t("chain.moveUp"), index === 0, () => moveRule(index, index - 1)),
        mkRuleIcon("caret-down", t("chain.moveDown"), index === rules.length - 1, () =>
          moveRule(index, index + 1)
        ),
        sep
      );
      const remove = mkRuleIcon("close", t("chain.removeRule"), false, () => {
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

      head.append(grip, toggle, n, kind, scope, spacer, acts);
      card.append(head);

      // ---- body (per-kind); diacritics is header-only ----
      if (rule.kind === "replace") {
        const b = document.createElement("div");
        b.className = "rule-body";
        const fields = document.createElement("div");
        fields.className = "rule-fields";
        const from = createComponentElement("input");
        from.type = "text";
        from.placeholder = t("chain.find");
        from.value = rule.from;
        from.spellcheck = false;
        from.addEventListener("input", () => (rule.from = from.value));
        const to = createComponentElement("input");
        to.type = "text";
        // A read-only built-in says what an empty replacement does (#556).
        to.placeholder = (inline ? el(ids.set)?.dataset.readonly === "true" : el(ids.set)?.disabled) ? t("chain.replaceNothing") : t("chain.replaceWith");
        to.value = rule.to;
        to.spellcheck = false;
        to.addEventListener("input", () => (rule.to = to.value));

        // find → replace as two input groups (#556); the flags change how the
        // find text is read, so they sit inside its field.
        const group = (input, tools) => {
          const box = document.createElement("div");
          box.className = "field sm";
          box.append(input);
          if (tools.length) {
            const holder = document.createElement("div");
            holder.className = "field-tools";
            holder.append(...tools);
            box.append(holder);
          }
          return box;
        };
        const flags = ids.rules === "pe-rules" ? [] : [
          ["regex", ".*", t("chain.flagRegexTitle"), ""],
          ["case_sensitive", "Aa", t("chain.flagMatchCaseTitle"), ""],
          ["whole_word", "ab", t("chain.flagWholeWordTitle"), "flag-word"],
        ].map(([key, text, hint, extra]) => {
          const flag = createComponentElement("button");
          flag.type = "button";
          flag.className = `field-flag ${extra}`.trim();
          flag.textContent = text;
          flag.title = hint;
          flag.setAttribute("aria-label", hint);
          flag.setAttribute("aria-pressed", String(!!rule[key]));
          flag.addEventListener("click", () => {
            rule[key] = !rule[key];
            flag.setAttribute("aria-pressed", String(rule[key]));
          });
          return flag;
        });
        const arrow = document.createElement("span");
        arrow.className = "rule-arrow";
        arrow.setAttribute("aria-hidden", "true");
        arrow.textContent = "→";
        // The preset editor exposes both meanings even after values replace
        // their placeholders. Other chain instances keep their compact layout.
        const labelledGroup = (input, tools, key) => {
          const box = group(input, tools);
          if (ids.rules !== "pe-rules") return box;
          const label = document.createElement("div");
          label.className = "pe-rule-field";
          const caption = document.createElement("label");
          input.id = `${ids.rules}-${rule.id}-${key === "chain.find" ? "find" : "replace"}`;
          caption.htmlFor = input.id;
          caption.className = "pe-rule-field-label";
          caption.textContent = t(key);
          input.setAttribute("aria-label", t(key));
          label.append(caption, box);
          return label;
        };
        fields.append(labelledGroup(from, flags, "chain.find"), arrow, labelledGroup(to, [], "chain.replaceWith"));
        b.append(fields);
        if (ids.rules === "pe-rules") {
          const options = document.createElement("div");
          options.className = "pe-match-options";
          const makeOption = (key, labelKey, hintKey) => {
            const label = document.createElement("label"); label.className = "pe-match-option";
            const input = createComponentElement("input", { type: "checkbox" });
            input.className = "pe-option-" + key; input.checked = !!rule[key];
            input.setAttribute("aria-label", t(labelKey));
            input.addEventListener("change", () => { rule[key] = input.checked; });
            const copy = document.createElement("span");
            const name = document.createElement("span"); name.textContent = t(labelKey); copy.append(name);
            if (hintKey) { const help = document.createElement("small"); help.textContent = t(hintKey); copy.append(help); }
            label.append(input, copy); return label;
          };
          options.append(makeOption("case_sensitive", "presets.matchCase", "presets.matchCaseHelp"), makeOption("whole_word", "presets.wholeWords", "presets.wholeWordsHelp"));
          const advanced = document.createElement("details"); advanced.className = "pe-pattern-options"; advanced.open = !!rule.regex;
          const summary = document.createElement("summary"); summary.textContent = t("presets.advanced");
          advanced.append(summary, makeOption("regex", "presets.pattern", "presets.patternHelp"));
          options.append(advanced);
          const updateOptions = () => { options.hidden = !from.value && !rule.regex && !rule.case_sensitive && !rule.whole_word; };
          from.addEventListener("input", updateOptions); updateOptions(); b.append(options);
        }
        card.append(b);
      } else if (rule.kind === "case") {
        const b = document.createElement("div");
        b.className = "rule-body";
        const seg = document.createElement("div");
        seg.className = "seg";
        for (const [value, text] of [
          ["title", t("chain.caseTitle")],
          ["lower", t("chain.caseLower")],
          ["upper", t("chain.caseUpper")],
          ["sentence", t("chain.caseSentence")],
        ]) {
          const btn = createComponentElement("button");
          btn.type = "button";
          btn.className = "seg-btn" + (rule.style === value ? " active" : "");
          btn.textContent = text;
          btn.addEventListener("click", () => {
            rule.style = value;
            seg.querySelectorAll(".seg-btn").forEach((s) => s.classList.toggle("active", s === btn));
            small.hidden = value !== "title";
          });
          seg.appendChild(btn);
        }
        const note = document.createElement("span");
        note.className = "rule-note";
        note.textContent = t("chain.caseNote");
        // Small words are title case's alone (#537).
        const small = smallWordsControl(rule);
        small.hidden = rule.style !== "title";
        if (inline) {
          const label = document.createElement("label"); label.className = "pe-rule-field";
          const caption = document.createElement("span"); caption.className = "pe-rule-field-label"; caption.textContent = t("presets.letterCase");
          const pick = document.createElement("select"); pick.className = "pe-case-style"; pick.setAttribute("aria-label", t("presets.letterCase"));
          for (const value of ["title", "lower", "upper", "sentence"]) { const option = document.createElement("option"); option.value = value; option.textContent = t("presets.case." + value); pick.append(option); }
          pick.value = rule.style;
          pick.addEventListener("change", () => { rule.style = pick.value; small.hidden = pick.value !== "title"; note.hidden = pick.value !== "title"; });
          note.hidden = rule.style !== "title";
          label.append(caption, pick); b.append(label, note, small);
        } else b.append(seg, note, small);
        card.append(b);
      } else if (rule.kind === "key") {
        const b = document.createElement("div");
        b.className = "rule-body";
        const seg = document.createElement("div");
        seg.className = "seg";
        for (const [value, text] of [
          ["camelot", "Camelot"],
          ["openkey", "Open Key"],
          ["musical", t("chain.keyMusical")],
        ]) {
          const btn = createComponentElement("button");
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
        if (inline) {
          const label = document.createElement("label"); label.className = "pe-rule-field";
          const caption = document.createElement("span"); caption.className = "pe-rule-field-label"; caption.textContent = t("presets.keyFormat");
          const pick = document.createElement("select"); pick.setAttribute("aria-label", t("presets.keyFormat"));
          for (const [value, text] of [["camelot", "Camelot — 8A"], ["openkey", "Open Key — 1m"], ["musical", t("chain.keyMusical") + " — Am"]]) { const option = document.createElement("option"); option.value = value; option.textContent = text; pick.append(option); }
          pick.value = rule.style; pick.addEventListener("change", () => { rule.style = pick.value; });
          label.append(caption, pick); b.append(label, note);
        } else b.append(seg, note);
        card.append(b);
      } else if (rule.kind === "untransliterate") {
        // The one step whose losses are worth stating on the card: reversing a
        // romanization can't recover what the forward direction dropped, and the
        // per-word guard is the reason English text survives it.
        const b = document.createElement("div");
        b.className = "rule-body";
        const note = document.createElement("span");
        note.className = "rule-note";
        note.textContent = t("chain.untranslitNote");
        b.append(note);
        card.append(b);
      }

      if (inline) {
        const editor = document.createElement("div"); editor.className = "pe-inline-editor"; editor.id = `pe-edit-${rule.id}`;
        const inside = document.createElement("div"); inside.className = "pe-inline-content";
        const tools = document.createElement("div"); tools.className = "pe-rule-tools";
        const targetLabel = document.createElement("label"); targetLabel.className = "pe-rule-field pe-target-field";
        const caption = document.createElement("span"); caption.className = "pe-rule-field-label"; caption.textContent = t("presets.appliesTo");
        targetLabel.append(caption, scope); tools.append(targetLabel, acts); inside.append(tools);
        inside.append(...Array.from(card.children).filter(child => child !== head));
        if (!inside.querySelector(".rule-body")) { const note = document.createElement("p"); note.className = "rule-note"; note.textContent = describeRule(rule, t).help; inside.append(note); }
        const error = document.createElement("p"); error.className = "pe-rule-error"; error.id = `pe-error-${rule.id}`; error.hidden = true; error.setAttribute("role", "alert"); inside.append(error);
        const open = createComponentElement("button"); open.type = "button"; open.className = "pe-rule-open"; open.setAttribute("aria-controls", editor.id);
        const title = document.createElement("span"); title.className = "pe-rule-title";
        const help = document.createElement("span"); help.className = "pe-rule-help";
        const text = document.createElement("span"); text.className = "pe-rule-copy"; text.append(title, help);
        const target = document.createElement("span"); target.className = "pe-rule-target";
        const caret = document.createElement("span"); caret.className = "pe-rule-caret"; caret.innerHTML = ico("caret-down");
        open.append(n, text, target, caret);
        head.replaceChildren(...(readOnly ? [] : [grip, toggle]), open);
        editor.append(inside); card.append(editor);
        const setExpanded = value => {
          card.classList.toggle("is-expanded", value); editor.inert = !value; editor.setAttribute("aria-hidden", String(!value));
          open.setAttribute("aria-expanded", String(value));
        };
        setExpanded(rule.id === expandedRule);
        open.addEventListener("click", () => {
          expandedRule = expandedRule === rule.id ? null : rule.id;
          for (const sibling of body.children) {
            const expanded = sibling.dataset.key === String(expandedRule);
            sibling.classList.toggle("is-expanded", expanded);
            sibling.querySelector(".pe-inline-editor").inert = !expanded;
            sibling.querySelector(".pe-inline-editor").setAttribute("aria-hidden", String(!expanded));
            sibling.querySelector(".pe-rule-open").setAttribute("aria-expanded", String(expanded));
          }
        });
        // A built-in's summary stays operable; only the actual editing controls
        // are disabled. A disabled outer fieldset would also disable disclosure.
        const readonly = document.createElement("fieldset"); readonly.className = "pe-rule-fields-set"; readonly.disabled = readOnly;
        readonly.append(...inside.childNodes); inside.append(readonly); toggle.disabled = readonly.disabled;
        if (readonly.disabled) for (const control of readonly.querySelectorAll("input, select, button, textarea")) control.disabled = true;
      }
      body.append(card);
    });
    if (inline) {
      refreshPresentation();
      if (focusKey && focusLabel) requestAnimationFrame(() => {
        if (interactionRevision !== revisionAtRender) return;
        const card = Array.from(body.children).find(node => node.dataset.key === focusKey);
        const control = card && Array.from(card.querySelectorAll("[aria-label]")).find(node => node.getAttribute("aria-label") === focusLabel && !node.disabled);
        (control || card?.querySelector(".pe-rule-open") || body.querySelector(".pe-rule-open"))?.focus({ preventScroll: true });
      });
    }
  }

  function refreshPresentation() {
    if (!inline) return;
    const scopes = new Set(rules.map(rule => rule.scope || "tags"));
    const sharedTarget = el("pe-rules-scope");
    sharedTarget.hidden = scopes.size !== 1;
    sharedTarget.textContent = scopes.size === 1
      ? el(ids.rules).querySelector(".rule-scope")?.selectedOptions[0]?.textContent || [...scopes][0]
      : "";
    for (const card of el(ids.rules).children) {
      const rule = rules.find(rule => String(rule.id) === card.dataset.key);
      if (!rule) continue;
      const info = describeRule(rule, t);
      card.querySelector(".pe-rule-title").textContent = info.title;
      card.querySelector(".pe-rule-help").textContent = info.help;
      const target = card.querySelector(".pe-rule-target");
      target.textContent = card.querySelector(".rule-scope")?.selectedOptions[0]?.textContent || rule.scope;
      target.hidden = scopes.size === 1;
    }
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
    refreshPresentation,
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
      expandedRule = null;
      rules = (group.rules || []).map((r) => ({
        id: ++ruleIdCounter,
        ...ruleForGroup(r),
        scope: r.scope || group.scope || "tags",
      }));
      render();
    },
  };

  if (ids.add) el(ids.add).addEventListener("click", () => addRule());
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
function createPresetChecklist({ menu, context, isActive, setActive, appliesTo, ruleCount, onOpen }) {
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

    const tick = createComponentElement("input", { type: "checkbox" });
    tick.className = "group-tick";
    tick.checked = isActive(group);
    // Each job has its own set (#555): a preset with nothing for this one can't
    // be ticked here. One already ticked can still be unticked.
    tick.disabled = !applies && !tick.checked;
    tick.addEventListener("change", () => setActive(group, tick.checked));

    // tick · name ……… target · rules for this job (or "not for this job") · ›
    // (#556): what runs is the set of bold names, and › opens the preset.
    const name = document.createElement("span");
    name.className = "preset-apply";
    name.textContent = presetLabel(group);
    const scope = document.createElement("span");
    scope.className = "group-scope";
    scope.textContent = targetLabel(group);
    const count = document.createElement("span");
    count.className = "group-rules";
    count.textContent = applies ? tn("unit.rule", ruleCount(group, context())) : t("presets.notForJob");
    const open = createComponentElement("button");
    open.type = "button";
    open.className = "icon preset-open";
    open.innerHTML = ico("chevron-right");
    open.title = t("presets.openOne");
    open.setAttribute("aria-label", open.title);
    open.addEventListener("click", (e) => {
      e.preventDefault();
      onOpen(group);
    });

    const note = presetNote(group);
    const summary = note ? `${note}\n${groupSummary(group)}` : groupSummary(group);
    label.title = applies ? summary : `${summary}\n${t("presets.notHere")}`;
    label.append(tick, name, scope, count, open);
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
      // A note, not one more heading (#569): muted text, no small caps.
      empty.className = "preset-none";
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
