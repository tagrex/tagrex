// One rule chain PER CONTEXT (#236).
//
// A single shared chain was wrong in a way that only shows up in use: RENAMER
// almost always wants a space turned into an underscore, and FROM NAME wants
// exactly the opposite, so whichever was set last quietly ruined the other.
// The rules belong to the job, not to the app.
//
// Four jobs have one: importing a release, reading tags out of a file name,
// renaming, and the general-purpose chain in GENERATOR. EDITOR has none on
// purpose — typing a value by hand is the one place where the app must not
// second-guess what was typed, alternating capitals and all.
//
// What is stored here is the SERIALIZED chain (rules + scope). The live one is
// the block's DOM, which there is only one of and which moves between panels
// (#149, #233) — so switching context saves the block's chain into this store
// and loads the incoming one back into the block. Groups are not per context:
// the saved shelf is a library to load FROM, and a library that differed by
// where you stood would be a worse library.
import { invoke } from "./invoke.js";

const STORAGE_KEY = "tagrex.chains";

// The contexts, and the label each uses when it says what it is about to do.
const CHAIN_CONTEXTS = {
  online: "the imported values",
  fromname: "the tags read from the name",
  renamer: "the new names",
  generator: "the selection",
};

// { [context]: { rules: [...], scope: "tags" } }
let chains = load();

// generator.js owns the one LIVE chain — the block's DOM — and registers it
// here. When the context being asked about is the one currently on screen, that
// DOM is the truth and this store is a keystroke behind it; every other context
// is stored and nothing else can be.
let liveChain = () => null;
function setLiveChainSource(fn) {
  liveChain = fn;
}

function load() {
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) || "{}");
    const out = {};
    for (const key of Object.keys(CHAIN_CONTEXTS)) {
      const chain = raw[key];
      if (chain && Array.isArray(chain.rules)) out[key] = chain;
    }
    return out;
  } catch (e) {
    return {};
  }
}

function persist() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(chains));
  } catch (e) {
    /* localStorage unavailable — the chains just won't survive a restart */
  }
}

// What to redo when a context's chain changes (#248). The panels register their
// own "show me again" — the read-out under a pattern, the staged plan that came
// from this very context — because changing a rule and then having to press the
// button again to see it is the two-step this whole design got rid of.
const listeners = new Map();
function onChainChanged(context, fn) {
  if (!listeners.has(context)) listeners.set(context, []);
  listeners.get(context).push(fn);
}
function notifyChainChanged(context) {
  for (const fn of listeners.get(context) || []) fn();
}

// The chain a context holds, or an empty one. Never null: every caller wants
// something with `.rules`.
function chainFor(context) {
  const live = liveChain();
  if (live && live.context === context) return live.chain;
  return storedChainFor(context);
}

// The stored chain, ignoring whatever is live. Which is what a context SWITCH
// has to read: by the time it loads the incoming chain it has already declared
// the incoming context to be the one on screen, so asking the live-aware reader
// would hand back the outgoing rules and carry them into the new context —
// exactly the leak between RENAMER and FROM NAME this file exists to prevent.
function storedChainFor(context) {
  return chains[context] || { rules: [], scope: "tags" };
}

function setChainFor(context, chain) {
  if (!CHAIN_CONTEXTS[context]) return;
  if (chain.rules.length) chains[context] = chain;
  else delete chains[context];
  persist();
}

function chainHasRules(context) {
  return chainFor(context).rules.length > 0;
}

// Run a context's chain over a plan just built, and give back the revised plan
// — or the plan untouched when there is nothing to run (#237).
//
// This is what makes the chain part of the operation instead of a second thing
// to remember: one press produces the values and cleans them up, one Apply and
// one undo entry, in the order a person would do it by hand. A failure returns
// the original plan rather than nothing: the values are still worth showing,
// and a broken rule is not a reason to lose the work of the step before it.
async function runChainOverPlan(plan, context) {
  if (!plan || !plan.changes?.length || !chainHasRules(context)) return plan;
  const chain = chainFor(context);
  // A run is as good a moment as any to write down what it ran, so a chain
  // that was typed and used but never switched away from still survives a
  // restart.
  setChainFor(context, chain);
  try {
    return await invoke("preview_transform_over_plan", {
      plan,
      groups: [{ name: "", scope: chain.scope, rules: chain.rules }],
    });
  } catch (e) {
    return plan;
  }
}

export {
  CHAIN_CONTEXTS,
  onChainChanged,
  notifyChainChanged,
  chainFor,
  storedChainFor,
  setChainFor,
  setLiveChainSource,
  chainHasRules,
  runChainOverPlan,
};
