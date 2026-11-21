// Settings › AI assistants (#828): pair Claude Code or Claude Desktop with the
// open library over MCP, see each connection's setup again, remove it.
//
// Like the Beatport account, this is not part of SettingsDto: the backend owns
// the keys, so connecting and removing take effect at once rather than on Save.
import { el, escapeHtml, toast } from "./dom.js";
import { currentLang, t } from "./i18n.js";
import { invoke } from "./invoke.js";

const NAMES = { "claude-code": "Claude Code", "claude-desktop": "Claude Desktop" };

// The last list the backend gave, redrawn on a language change.
let overview = null;
// The connection just made: its card opens scrolled into view.
let fresh = "";

function when(secs) {
  return new Date(secs * 1000).toLocaleString(currentLang(), {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function setupHint(kind) {
  if (kind === "claude-code") return t("settings.assistants.runThis");
  return overview.desktop_config
    ? t("settings.assistants.addThis", { file: overview.desktop_config })
    : t("settings.assistants.addThisAnywhere");
}

function render() {
  const list = el("mcp-clients");
  if (!overview) return;
  if (!overview.clients.length) {
    list.innerHTML = `<p class="settings-hint">${escapeHtml(t("settings.assistants.none"))}</p>`;
    return;
  }
  list.innerHTML = overview.clients
    .map((client) => {
      const used = client.last_used
        ? t("settings.assistants.lastUsed", { date: when(client.last_used) })
        : t("settings.assistants.neverUsed");
      return `<div class="mcp-client${client.id === fresh ? " fresh" : ""}" data-id="${escapeHtml(client.id)}">
        <div class="mcp-client-head">
          <span class="mcp-client-name">${escapeHtml(NAMES[client.kind] || client.kind)}</span>
          <span class="settings-hint mcp-client-when">${escapeHtml(t("settings.assistants.created", { date: when(client.created) }))} · ${escapeHtml(used)}</span>
          <button class="text-btn mcp-copy" type="button">${escapeHtml(t("settings.assistants.copy"))}</button>
          <button class="text-btn mcp-remove" type="button">${escapeHtml(t("settings.assistants.remove"))}</button>
        </div>
        <p class="settings-hint">${escapeHtml(setupHint(client.kind))}</p>
        <pre class="mcp-setup">${escapeHtml(client.setup)}</pre>
      </div>`;
    })
    .join("");
  const card = fresh && list.querySelector(".mcp-client.fresh");
  if (card) card.scrollIntoView({ block: "nearest" });
}

async function refreshAssistants() {
  try {
    overview = await invoke("mcp_clients", {});
  } catch (e) {
    overview = { clients: [], desktop_config: "" };
  }
  render();
}

async function pair(kind) {
  const buttons = [el("mcp-pair-code"), el("mcp-pair-desktop")];
  buttons.forEach((b) => (b.disabled = true));
  try {
    overview = await invoke("mcp_pair", { kind });
    const added = overview.clients[overview.clients.length - 1];
    fresh = added ? added.id : "";
    render();
  } catch (e) {
    toast(String(e), true);
  } finally {
    buttons.forEach((b) => (b.disabled = false));
  }
}

async function copy(text) {
  try {
    await navigator.clipboard.writeText(text);
  } catch (e) {
    // Older engines: copy through a selection.
    const area = document.createElement("textarea");
    area.value = text;
    document.body.append(area);
    area.select();
    document.execCommand("copy");
    area.remove();
  }
  toast(t("settings.assistants.copied"));
}

el("mcp-pair-code").addEventListener("click", () => pair("claude-code"));
el("mcp-pair-desktop").addEventListener("click", () => pair("claude-desktop"));
el("mcp-clients").addEventListener("click", async (event) => {
  const card = event.target.closest(".mcp-client");
  if (!card) return;
  const client = overview.clients.find((c) => c.id === card.dataset.id);
  if (!client) return;
  if (event.target.closest(".mcp-copy")) {
    await copy(client.setup);
  } else if (event.target.closest(".mcp-remove")) {
    try {
      overview = await invoke("mcp_revoke", { id: client.id });
      if (fresh === client.id) fresh = "";
      render();
      toast(t("settings.assistants.removed"));
    } catch (e) {
      toast(String(e), true);
    }
  }
});

export { refreshAssistants, render as retranslateAssistants };
