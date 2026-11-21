//! Tag edits (#830) and renames (#835) an assistant proposes and a person
//! applies.
//!
//! The window stages a proposal in its ordinary preview; nothing is written
//! here. A proposal travels as an event to the interface, which answers with
//! [`mcp_proposal_reply`] whether it could stage it, and later with
//! [`mcp_proposal_outcome`] whether the person applied or discarded it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Emitter, Manager, State};

use tagrex_commands::ErrorDto;

use super::store::{self, Client};
use super::{lookup, tools};
use crate::{shell_error, AppState, SESSION_QUEUE};

/// Emitted with a proposal for the interface to stage.
const PROPOSAL_EVENT: &str = "tagrex://agent-proposal";
/// How long the interface has to say whether it staged a proposal. An import
/// (#839) fetches the release cover first, so it is generous.
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);
/// Outcomes kept for `proposal_status`; the oldest go first.
const KEEP: usize = 200;

/// What the interface says when a proposal reaches it.
#[derive(Debug, Deserialize)]
pub struct ReplyDto {
    staged: bool,
    /// Why not, when not: `"pending"` (the person has changes of their own
    /// staged), `"nothing"` (no change left) or a message.
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    files: usize,
    #[serde(default)]
    changes: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Status {
    Staged,
    Applied { files: usize },
    Discarded,
}

#[derive(Default)]
struct Inner {
    waiting: HashMap<String, tokio::sync::oneshot::Sender<ReplyDto>>,
    /// Proposal id -> (client id, status), in the order they were staged.
    known: Vec<(String, String, Status)>,
}

#[derive(Default)]
pub struct Proposals(Mutex<Inner>);

impl Proposals {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The assistant's name as the interface shows it.
fn assistant_name(kind: &str) -> &str {
    match kind {
        "claude-code" => "Claude Code",
        "claude-desktop" => "Claude Desktop",
        other => other,
    }
}

/// `propose_tag_edits` for `client`.
pub async fn propose(
    app: &tauri::AppHandle,
    client: &Client,
    arguments: Value,
) -> Result<Value, ErrorDto> {
    let state = app.state::<AppState>().inner().clone();
    let prepared = SESSION_QUEUE
        .run(move || tools::prepare_proposal(&state, arguments))
        .await?;
    let mut answer = prepared.report.clone();
    if prepared.edits.is_empty() {
        answer["staged"] = json!(false);
        answer["reason"] = json!("nothing would change");
        return Ok(answer);
    }
    let edits: Vec<Value> = prepared
        .edits
        .iter()
        .map(|edit| {
            json!({
                "path": edit.path,
                "field": edit.field,
                "value": edit.value.clone().unwrap_or_default(),
            })
        })
        .collect();
    let payload = json!({
        "kind": "tags",
        "assistant": assistant_name(&client.kind),
        "summary": prepared.summary,
        "edits": edits,
    });
    stage(app, client, answer, payload).await
}

/// `propose_rename` for `client` (#835): the RENAMER's plan, staged as is.
pub async fn propose_rename(
    app: &tauri::AppHandle,
    client: &Client,
    arguments: Value,
) -> Result<Value, ErrorDto> {
    let state = app.state::<AppState>().inner().clone();
    let prepared = SESSION_QUEUE
        .run(move || tools::prepare_rename_proposal(&state, arguments))
        .await?;
    let mut answer = prepared.report.clone();
    if prepared.plan.changes.is_empty() {
        answer["staged"] = json!(false);
        answer["reason"] = json!("nothing would change");
        return Ok(answer);
    }
    let payload = json!({
        "kind": "rename",
        "assistant": assistant_name(&client.kind),
        "summary": prepared.summary,
        "plan": prepared.plan,
    });
    stage(app, client, answer, payload).await
}

/// `propose_import` for `client` (#839): the release and the pairs go to the
/// window, which builds the import with the ONLINE panel's own code.
pub async fn propose_import(
    app: &tauri::AppHandle,
    client: &Client,
    arguments: Value,
) -> Result<Value, ErrorDto> {
    let args = tools::parse_import(arguments)?;
    let release = lookup::fetch_release(app, &args.source, args.release_id.trim()).await?;
    let state = app.state::<AppState>().inner().clone();
    let listed = SESSION_QUEUE
        .run(move || tools::listed_paths(&state))
        .await?;
    let source = args.source.clone();
    let resolved = tools::resolve_import(&release, &listed, args)?;
    let mut answer = resolved.report.clone();
    if resolved.pairs.is_empty() {
        answer["staged"] = json!(false);
        answer["reason"] = json!("no file could be paired with a track of this release");
        return Ok(answer);
    }
    let pairs: Vec<Value> = resolved
        .pairs
        .iter()
        .map(|(path, track)| json!({ "path": path, "track": track }))
        .collect();
    let payload = json!({
        "kind": "import",
        "assistant": assistant_name(&client.kind),
        "summary": resolved.summary,
        "source": source,
        "release": release,
        "pairs": pairs,
        "catalog_number": resolved.catalog_number,
    });
    stage(app, client, answer, payload).await
}

/// Send a checked proposal to the window and report what it did with it.
async fn stage(
    app: &tauri::AppHandle,
    client: &Client,
    mut answer: Value,
    mut payload: Value,
) -> Result<Value, ErrorDto> {
    let id = store::random_hex(8).map_err(shell_error)?;
    payload["id"] = json!(id);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let proposals = app.state::<Proposals>();
    proposals.lock().waiting.insert(id.clone(), sender);
    let sent = app.emit(PROPOSAL_EVENT, payload);
    let reply = match sent {
        Ok(()) => tokio::time::timeout(REPLY_TIMEOUT, receiver)
            .await
            .ok()
            .and_then(Result::ok),
        Err(_) => None,
    };
    proposals.lock().waiting.remove(&id);
    let Some(reply) = reply else {
        return Err(ErrorDto::plain(
            "TagRex did not take the proposal. Ask the user to bring the TagRex window up and try again.",
        ));
    };
    answer["staged"] = json!(reply.staged);
    answer["files"] = json!(reply.files);
    answer["changes"] = json!(reply.changes);
    if reply.staged {
        let mut inner = proposals.lock();
        inner
            .known
            .push((id.clone(), client.id.clone(), Status::Staged));
        if inner.known.len() > KEEP {
            inner.known.remove(0);
        }
        drop(inner);
        answer["proposal"] = json!(id);
        answer["note"] = json!(
            "Staged in TagRex's preview. Nothing is written until the user clicks Apply; use proposal_status to see what they did."
        );
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.request_user_attention(Some(tauri::UserAttentionType::Informational));
        }
    } else {
        answer["reason"] = json!(match reply.reason.as_deref() {
            Some("pending") => "TagRex already has unapplied changes on screen (the user's own or an earlier proposal). Ask the user to apply or discard them first, then propose again.".to_string(),
            Some("nothing") | None => "nothing would change".to_string(),
            Some(other) => other.to_string(),
        });
    }
    Ok(answer)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusArgs {
    proposal: String,
}

/// `proposal_status` for `client`: only its own proposals are known to it.
pub fn status(
    app: &tauri::AppHandle,
    client: &Client,
    arguments: Value,
) -> Result<Value, ErrorDto> {
    let args: StatusArgs = serde_json::from_value(arguments)
        .map_err(|error| ErrorDto::plain(format!("invalid arguments: {error}")))?;
    let proposals = app.state::<Proposals>();
    let inner = proposals.lock();
    let found = inner
        .known
        .iter()
        .find(|(id, owner, _)| *id == args.proposal && *owner == client.id);
    match found {
        Some((_, _, status)) => serde_json::to_value(status).map_err(shell_error),
        None => Ok(json!({ "status": "unknown" })),
    }
}

/// The interface's answer to a proposal event.
#[tauri::command]
pub fn mcp_proposal_reply(state: State<'_, Proposals>, id: String, reply: ReplyDto) {
    if let Some(sender) = state.lock().waiting.remove(&id) {
        let _ = sender.send(reply);
    }
}

/// The person applied (`files` of them) or discarded a staged proposal.
#[tauri::command]
pub fn mcp_proposal_outcome(state: State<'_, Proposals>, id: String, applied: Option<usize>) {
    let mut inner = state.lock();
    if let Some(entry) = inner.known.iter_mut().find(|(known, _, _)| *known == id) {
        entry.2 = match applied {
            Some(files) => Status::Applied { files },
            None => Status::Discarded,
        };
    }
}
