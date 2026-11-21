//! The window's side of the channel (#828): listens on a local socket, checks
//! each call's secret and runs the tool on the session queue.
//!
//! Unix: a socket file in the owner-only `mcp` folder, itself owner-only.
//! Windows: a named pipe under a random name that refuses remote clients. The
//! address goes into `endpoint.json` beside the credentials, where the
//! connector looks for it.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio::io::{AsyncRead, AsyncWrite};

use super::store::{self, Store};
use super::{lookup, proposals, tools, wire};
use crate::{AppState, SESSION_QUEUE};

/// A connector that has not sent its request by then is dropped.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Calls handled at once; more wait for a free slot.
const MAX_CONNECTIONS: usize = 16;

static STARTED: AtomicBool = AtomicBool::new(false);

/// Where the window listens, as written to `endpoint.json`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Endpoint {
    pub v: u32,
    /// `"unix"` or `"pipe"`.
    pub transport: String,
    pub address: String,
    pub pid: u32,
}

/// Start listening, once per process. Called at startup when an assistant is
/// paired, and again on every pairing; the later calls do nothing.
pub fn start(app: tauri::AppHandle, store: Arc<Store>) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(error) = serve(app, store).await {
            eprintln!("tagrex: the assistant connection stopped: {error}");
            STARTED.store(false, Ordering::SeqCst);
        }
    });
}

fn write_endpoint(store: &Store, transport: &str, address: String) -> io::Result<()> {
    let endpoint = Endpoint {
        v: wire::PROTOCOL,
        transport: transport.to_string(),
        address,
        pid: std::process::id(),
    };
    store::write_private(
        &store.endpoint_path(),
        &serde_json::to_vec_pretty(&endpoint)?,
    )
}

#[cfg(unix)]
async fn serve(app: tauri::AppHandle, store: Arc<Store>) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    store::ensure_private_dir(store.dir())?;
    let socket = store.dir().join("tagrex.sock");
    // Left by a run that did not exit cleanly; one window per profile, so
    // nothing else is listening on it.
    match std::fs::remove_file(&socket) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let listener = tokio::net::UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    write_endpoint(&store, "unix", socket.to_string_lossy().into_owned())?;
    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (stream, _) = listener.accept().await?;
        let permit = slots
            .clone()
            .acquire_owned()
            .await
            .map_err(io::Error::other)?;
        let (app, store) = (app.clone(), store.clone());
        tauri::async_runtime::spawn(async move {
            handle(stream, app, store).await;
            drop(permit);
        });
    }
}

#[cfg(windows)]
async fn serve(app: tauri::AppHandle, store: Arc<Store>) -> io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;
    store::ensure_private_dir(store.dir())?;
    let name = format!(r"\\.\pipe\tagrex-mcp-{}", store::random_hex(16)?);
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&name)?;
    write_endpoint(&store, "pipe", name.clone())?;
    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        server.connect().await?;
        let connected = server;
        server = ServerOptions::new()
            .reject_remote_clients(true)
            .create(&name)?;
        let permit = slots
            .clone()
            .acquire_owned()
            .await
            .map_err(io::Error::other)?;
        let (app, store) = (app.clone(), store.clone());
        tauri::async_runtime::spawn(async move {
            handle(connected, app, store).await;
            drop(permit);
        });
    }
}

async fn handle<S>(mut stream: S, app: tauri::AppHandle, store: Arc<Store>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let body = match tokio::time::timeout(
        REQUEST_TIMEOUT,
        wire::read_frame_async(&mut stream, wire::MAX_REQUEST),
    )
    .await
    {
        Ok(Ok(body)) => body,
        _ => return,
    };
    let response = respond(&body, &app, store).await;
    let mut bytes = serde_json::to_vec(&response).unwrap_or_default();
    if bytes.len() > wire::MAX_RESPONSE {
        bytes = serde_json::to_vec(&wire::Response::Error {
            message: "the result is too large; ask for fewer files at a time".into(),
        })
        .unwrap_or_default();
    }
    let _ = wire::write_frame_async(&mut stream, &bytes).await;
}

async fn respond(body: &[u8], app: &tauri::AppHandle, store: Arc<Store>) -> wire::Response {
    let request: wire::Request = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(_) => {
            return wire::Response::Error {
                message: "malformed request".into(),
            }
        }
    };
    if request.v != wire::PROTOCOL {
        return wire::Response::Error {
            message: "this connection was set up by another TagRex version; restart the assistant"
                .into(),
        };
    }
    let (client, secret) = (request.client, request.secret);
    let authenticated =
        tauri::async_runtime::spawn_blocking(move || store.authenticate(&client, &secret)).await;
    let client = match authenticated {
        Ok(Ok(Some(client))) => client,
        Ok(Ok(None)) => return wire::Response::Unauthorized,
        Ok(Err(error)) => {
            return wire::Response::Error {
                message: format!("could not check the connection: {error}"),
            }
        }
        Err(error) => {
            return wire::Response::Error {
                message: error.to_string(),
            }
        }
    };
    if !tools::exists(&request.tool) {
        return wire::Response::Error {
            message: format!("unknown tool: {}", request.tool),
        };
    }
    let (tool, arguments) = (request.tool, request.arguments);
    // A proposal waits on the interface (#830); the rest only read the session.
    let outcome = match tool.as_str() {
        "propose_tag_edits" => proposals::propose(app, &client, arguments).await,
        "propose_rename" => proposals::propose_rename(app, &client, arguments).await,
        "proposal_status" => proposals::status(app, &client, arguments),
        // Online lookups and settings (#833, #834) need no library.
        "search_releases" => lookup::search(app, arguments).await,
        "read_release" => lookup::release(app, arguments).await,
        "read_settings" => lookup::settings(app).await,
        _ => {
            let state = app.state::<AppState>().inner().clone();
            SESSION_QUEUE
                .run(move || tools::call(&state, &tool, arguments))
                .await
        }
    };
    match outcome {
        Ok(result) => wire::Response::Ok { result },
        Err(error) => wire::Response::Error {
            message: error.text,
        },
    }
}
