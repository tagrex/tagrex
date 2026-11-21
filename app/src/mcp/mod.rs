//! Read access for AI assistants over MCP (#828).
//!
//! An assistant starts `tagrex --mcp-stdio --mcp-credential <file>`
//! ([`connector`]), which forwards each tool call to the running window
//! ([`host`]) over a private local channel ([`wire`]). Pairing ([`store`])
//! happens in Settings; the tools themselves are in [`tools`].

pub mod connector;
mod host;
pub mod proposals;
mod store;
mod tools;
mod wire;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{Manager, State};

use tagrex_commands::ErrorDto;

pub use proposals::Proposals;

use crate::shell_error;

/// The pairing store, once the settings folder is known (in `setup`).
#[derive(Default)]
pub struct McpState(Mutex<Option<Arc<store::Store>>>);

impl McpState {
    fn store(&self) -> Result<Arc<store::Store>, ErrorDto> {
        self.0
            .lock()
            .map_err(shell_error)?
            .clone()
            .ok_or_else(|| shell_error("the assistant connection is not available"))
    }
}

/// Whether this launch is the connector rather than the window.
pub fn is_connector(args: &[std::ffi::OsString]) -> bool {
    args.iter().any(|arg| arg == "--mcp-stdio")
}

/// At startup: find the pairing store and, when an assistant is paired, start
/// listening for it. Nothing listens until something has been paired.
pub fn install(app: &tauri::AppHandle) {
    let Ok(config) = app.path().app_config_dir() else {
        return;
    };
    let store = Arc::new(store::Store::new(config.join("mcp")));
    let paired = store.clients().is_ok_and(|clients| !clients.is_empty());
    *app.state::<McpState>().0.lock().unwrap() = Some(store.clone());
    if paired {
        host::start(app.clone(), store);
    }
}

#[derive(Serialize)]
pub struct McpClientDto {
    id: String,
    kind: String,
    created: u64,
    last_used: Option<u64>,
    /// What to paste into the assistant; it names the credential file, never
    /// the secret, so it can be shown again.
    setup: String,
}

#[derive(Serialize)]
pub struct McpOverviewDto {
    clients: Vec<McpClientDto>,
    /// Where Claude Desktop keeps its configuration on this system, for the
    /// hint next to its snippet. Empty where there is no Claude Desktop.
    desktop_config: String,
}

/// The program an assistant should start. An AppImage runs from a temporary
/// mount, so the AppImage file itself is named instead.
fn executable() -> Result<PathBuf, ErrorDto> {
    if let Some(image) = std::env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(image));
    }
    std::env::current_exe().map_err(shell_error)
}

/// The name the server goes by in the assistant: `tagrex`, or `tagrex-next`
/// for the side-by-side build (#818), so both can be connected.
fn server_name(app: &tauri::AppHandle) -> String {
    app.package_info()
        .name
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

/// A path quoted for the shell the command is pasted into: single quotes on
/// Unix, double quotes for PowerShell and cmd on Windows.
fn quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        format!("\"{text}\"")
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

fn setup_text(kind: &str, name: &str, exe: &Path, credential: &Path) -> String {
    if kind == "claude-code" {
        format!(
            "claude mcp add --scope user {name} -- {} --mcp-stdio --mcp-credential {}",
            quote(exe),
            quote(credential)
        )
    } else {
        let config = serde_json::json!({
            "mcpServers": {
                name: {
                    "command": exe.to_string_lossy(),
                    "args": ["--mcp-stdio", "--mcp-credential", credential.to_string_lossy()],
                }
            }
        });
        serde_json::to_string_pretty(&config).unwrap_or_default()
    }
}

fn desktop_config_path() -> String {
    if cfg!(target_os = "macos") {
        "~/Library/Application Support/Claude/claude_desktop_config.json".into()
    } else if cfg!(windows) {
        r"%APPDATA%\Claude\claude_desktop_config.json".into()
    } else {
        String::new()
    }
}

fn overview(app: &tauri::AppHandle, store: &store::Store) -> Result<McpOverviewDto, ErrorDto> {
    let exe = executable()?;
    let name = server_name(app);
    let clients = store
        .clients()
        .map_err(shell_error)?
        .into_iter()
        .map(|client| McpClientDto {
            setup: setup_text(
                &client.kind,
                &name,
                &exe,
                &store.credential_path(&client.id),
            ),
            id: client.id,
            kind: client.kind,
            created: client.created,
            last_used: client.last_used,
        })
        .collect();
    Ok(McpOverviewDto {
        clients,
        desktop_config: desktop_config_path(),
    })
}

/// The paired assistants, each with its setup text.
#[tauri::command]
pub async fn mcp_clients(
    app: tauri::AppHandle,
    state: State<'_, McpState>,
) -> Result<McpOverviewDto, ErrorDto> {
    let store = state.store()?;
    tauri::async_runtime::spawn_blocking(move || overview(&app, &store))
        .await
        .map_err(shell_error)?
}

/// Pair a new assistant of `kind` (`claude-code` or `claude-desktop`), start
/// listening if nothing was paired before, and return the new list. The new
/// client is the last one.
#[tauri::command]
pub async fn mcp_pair(
    app: tauri::AppHandle,
    state: State<'_, McpState>,
    kind: String,
) -> Result<McpOverviewDto, ErrorDto> {
    let store = state.store()?;
    tauri::async_runtime::spawn_blocking(move || {
        store.pair(&kind).map_err(shell_error)?;
        host::start(app.clone(), store.clone());
        overview(&app, &store)
    })
    .await
    .map_err(shell_error)?
}

/// Revoke an assistant: its next call is refused.
#[tauri::command]
pub async fn mcp_revoke(
    app: tauri::AppHandle,
    state: State<'_, McpState>,
    id: String,
) -> Result<McpOverviewDto, ErrorDto> {
    let store = state.store()?;
    tauri::async_runtime::spawn_blocking(move || {
        store.revoke(&id).map_err(shell_error)?;
        overview(&app, &store)
    })
    .await
    .map_err(shell_error)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_claude_code_command_quotes_paths_with_spaces() {
        let text = setup_text(
            "claude-code",
            "tagrex",
            Path::new("/Applications/TagRex.app/Contents/MacOS/tagrex"),
            Path::new("/Users/a/Library/Application Support/x/mcp/credentials/ab.json"),
        );
        if cfg!(windows) {
            assert!(
                text.contains("\"/Users/a/Library/Application Support/x/mcp/credentials/ab.json\"")
            );
        } else {
            assert_eq!(
                text,
                "claude mcp add --scope user tagrex -- '/Applications/TagRex.app/Contents/MacOS/tagrex' --mcp-stdio --mcp-credential '/Users/a/Library/Application Support/x/mcp/credentials/ab.json'"
            );
        }
    }

    #[test]
    fn the_desktop_snippet_is_a_config_with_one_server() {
        let text = setup_text(
            "claude-desktop",
            "tagrex-next",
            Path::new(r"C:\Program Files\TagRex Next\tagrex.exe"),
            Path::new(r"C:\Users\a\AppData\Roaming\com.tagrex.next\mcp\credentials\ab.json"),
        );
        let config: serde_json::Value = serde_json::from_str(&text).unwrap();
        let server = &config["mcpServers"]["tagrex-next"];
        assert_eq!(
            server["command"],
            r"C:\Program Files\TagRex Next\tagrex.exe"
        );
        assert_eq!(server["args"][0], "--mcp-stdio");
        assert_eq!(server["args"][1], "--mcp-credential");
    }

    #[cfg(unix)]
    #[test]
    fn a_quote_in_a_path_survives_the_shell() {
        assert_eq!(quote(Path::new("/a/it's")), r"'/a/it'\''s'");
    }

    #[test]
    fn the_connector_is_recognised_by_its_flag() {
        let args: Vec<std::ffi::OsString> = vec!["tagrex".into(), "--mcp-stdio".into()];
        assert!(is_connector(&args));
        assert!(!is_connector(&["tagrex".into(), "/music".into()]));
    }
}
