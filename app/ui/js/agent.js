// Tag edits (#830) and renames (#835) an AI assistant proposes over MCP.
//
// The backend checks a proposal and sends it here. Tag edits are staged in the
// same pending-edits buffer typed edits and imports use; a rename arrives as
// the plan the RENAMER builds for the same mask and is staged as it is. Either
// way the person reviews it in the ordinary diff and decides with Apply or
// Discard. Nothing an assistant sends is written by itself. A proposal never
// joins edits the person has staged: it is refused instead, and the assistant
// is told why.
import { el, escapeHtml, toast } from "./dom.js";
import { t, tn } from "./i18n.js";
import { hooks } from "./hooks.js";
import { invoke } from "./invoke.js";
import { edits, previewPlan, previewSource, setPreviewPlan, setPreviewSource } from "./state.js";
import { refreshFieldEditor } from "./editor.js";

const EVENT = "tagrex://agent-proposal";

// The proposal on screen: { id, assistant, summary }, or null.
let active = null;

function reply(id, answer) {
  invoke("mcp_proposal_reply", { id, reply: answer }).catch(() => {});
}

function paintBanner() {
  const banner = el("ab-agent");
  banner.hidden = !active;
  if (!active) return;
  const by = t("agent.proposedBy", { assistant: active.assistant });
  banner.innerHTML = `<svg class="ico"><use href="#i-sparkle"/></svg><span class="ab-agent-text">${escapeHtml(by)}: ${escapeHtml(active.summary)}</span>`;
  banner.title = `${by}: ${active.summary}`;
}

// The note that rides along with the plan into the history, so the batch says
// whose proposal it was. Added again whenever the plan is rebuilt, as it is
// when the person edits a staged value.
function decorateAgentPlan(plan) {
  if (!active || !plan) return;
  const notes = plan.notes || [];
  if (notes.some((n) => n.code === "plan.byAssistant")) return;
  plan.notes = [...notes, { code: "plan.byAssistant", args: { assistant: active.assistant } }];
}

async function receiveProposal(proposal) {
  const { id, assistant, summary } = proposal;
  // The person's own staged work comes first, and so does a proposal already
  // on screen: either one makes this a refusal, never a merge.
  if (previewPlan || edits.size) {
    reply(id, { staged: false, reason: "pending" });
    return;
  }
  if (proposal.kind === "rename") {
    stageRename(proposal);
    return;
  }
  for (const { path, field, value } of proposal.edits) {
    if (!edits.has(path)) edits.set(path, new Map());
    edits.get(path).set(field, value ?? "");
  }
  active = { id, assistant, summary };
  await hooks.previewEdits();
  if (!previewPlan || previewSource !== "edits") {
    active = null;
    edits.clear();
    reply(id, { staged: false, reason: "nothing" });
    return;
  }
  refreshFieldEditor();
  paintBanner();
  const files = previewPlan.changes.length;
  const changes = previewPlan.changes.reduce((n, c) => n + c.tag_changes.length, 0);
  // A field without a column of its own shows in a temporary one (#831).
  toast(t("agent.staged", { assistant, files: tn("unit.file", files) }));
  reply(id, { staged: true, files, changes });
}

// A rename is staged as the plan it came with: the backend built it with the
// RENAMER's own code, and the panel's mask and chain have no part in it.
function stageRename({ id, assistant, summary, plan }) {
  active = { id, assistant, summary };
  decorateAgentPlan(plan);
  setPreviewPlan(plan);
  setPreviewSource("agent");
  hooks.renderPreview(previewPlan);
  if (!previewPlan) {
    active = null;
    reply(id, { staged: false, reason: "nothing" });
    return;
  }
  paintBanner();
  const files = previewPlan.changes.length;
  toast(t("agent.staged", { assistant, files: tn("unit.file", files) }));
  reply(id, { staged: true, files, changes: files });
}

// The person applied the staged proposal, to `files` of its files.
function agentApplied(files) {
  if (!active) return;
  invoke("mcp_proposal_outcome", { id: active.id, applied: files }).catch(() => {});
  active = null;
  paintBanner();
}

// The staged plan went away without an Apply: discarded, or replaced by a
// library change. After agentApplied this does nothing.
function agentClosed() {
  if (!active) return;
  invoke("mcp_proposal_outcome", { id: active.id, applied: null }).catch(() => {});
  active = null;
  paintBanner();
}

const events = window.__TAURI__ && window.__TAURI__.event;
if (events) events.listen(EVENT, (e) => receiveProposal(e.payload));

export { agentApplied, agentClosed, decorateAgentPlan, paintBanner as retranslateAgent, receiveProposal };
