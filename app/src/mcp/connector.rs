//! `tagrex --mcp-stdio --mcp-credential <file>` (#828): the MCP server an
//! assistant starts. It speaks MCP (JSON-RPC 2.0, one message per line) on
//! stdin/stdout and hands each tool call to the running TagRex window over the
//! private channel. It never opens a library or a music file itself.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use super::host::Endpoint;
use super::store::Credential;
use super::{tools, wire};

/// Newest first; an unknown version a client asks for is answered with the
/// newest, as the specification says.
const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// How long one tool call may take. A hash scan of a large library is slow.
const CALL_TIMEOUT: Duration = Duration::from_secs(600);

const INSTRUCTIONS: &str = "TagRex is an audio tag editor. These tools read the library that is \
open in the TagRex window: they never change a file. Start with library_status; if no library is \
open, ask the user to open one in TagRex. Page through list_tracks rather than asking for \
everything at once, and use read_tracks for the full details of a few files.";

const NOT_RUNNING: &str =
    "TagRex is not running. Ask the user to open TagRex and a library in it, then try again.";
const REVOKED: &str = "This connection was removed in TagRex (Settings, AI assistants). \
Ask the user to connect the assistant again there.";

/// Run the connector until stdin closes; the process exit code.
pub fn run(args: impl Iterator<Item = std::ffi::OsString>) -> i32 {
    let Some(path) = credential_arg(args) else {
        eprintln!("tagrex: --mcp-stdio needs --mcp-credential <file>");
        return 2;
    };
    let connector = match Connector::from_credential(&path) {
        Ok(connector) => connector,
        Err(error) => {
            eprintln!("tagrex: cannot read {}: {error}", path.display());
            return 2;
        }
    };
    let stdin = io::stdin();
    let stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = connector.handle_line(&line) {
            let mut out = stdout.lock();
            let written = serde_json::to_writer(&mut out, &reply)
                .map_err(io::Error::from)
                .and_then(|()| out.write_all(b"\n"))
                .and_then(|()| out.flush());
            if written.is_err() {
                break;
            }
        }
    }
    0
}

fn credential_arg(args: impl Iterator<Item = std::ffi::OsString>) -> Option<PathBuf> {
    let mut args = args.skip_while(|arg| arg != "--mcp-credential");
    args.next()?;
    args.next().map(PathBuf::from)
}

/// Why a call did not reach a tool.
#[derive(Debug, PartialEq)]
enum CallError {
    NotRunning,
    Failed(String),
}

/// Sends one request to the window and returns its answer.
type Transport = Box<dyn Fn(&wire::Request) -> Result<wire::Response, CallError>>;

pub struct Connector {
    credential: Credential,
    transport: Transport,
}

impl Connector {
    fn from_credential(path: &Path) -> io::Result<Self> {
        let credential: Credential = serde_json::from_slice(&std::fs::read(path)?)?;
        // credentials/<id>.json sits in the mcp folder, beside endpoint.json.
        let dir = path
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no mcp folder"))?
            .to_path_buf();
        let credential_path = path.to_path_buf();
        Ok(Self {
            credential,
            transport: Box::new(move |request| send(&dir, &credential_path, request)),
        })
    }

    /// One incoming line; the line to answer with, if any.
    fn handle_line(&self, line: &str) -> Option<Value> {
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(_) => return Some(error_reply(Value::Null, -32700, "Parse error")),
        };
        if message.is_array() {
            return Some(error_reply(
                Value::Null,
                -32600,
                "Batches are not supported",
            ));
        }
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").cloned();
        match (method, id) {
            // A notification (no id) or a reply to something we never asked.
            (_, None) | (None, Some(_)) => None,
            (Some(method), Some(id)) => Some(self.handle_request(
                id,
                method,
                message.get("params").cloned().unwrap_or(Value::Null),
            )),
        }
    }

    fn handle_request(&self, id: Value, method: &str, params: Value) -> Value {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str);
                let version = asked
                    .filter(|v| PROTOCOL_VERSIONS.contains(v))
                    .unwrap_or(PROTOCOL_VERSIONS[0]);
                result_reply(
                    id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": {
                            "name": "tagrex",
                            "title": "TagRex",
                            "version": env!("CARGO_PKG_VERSION"),
                        },
                        "instructions": INSTRUCTIONS,
                    }),
                )
            }
            "ping" => result_reply(id, json!({})),
            "tools/list" => result_reply(id, json!({ "tools": tools::definitions() })),
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else {
                    return error_reply(id, -32602, "Missing tool name");
                };
                if !tools::exists(name) {
                    return error_reply(id, -32602, &format!("Unknown tool: {name}"));
                }
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                result_reply(id, self.call_tool(name, arguments))
            }
            _ => error_reply(id, -32601, &format!("Method not found: {method}")),
        }
    }

    fn call_tool(&self, name: &str, arguments: Value) -> Value {
        let request = wire::Request {
            v: wire::PROTOCOL,
            client: self.credential.client.clone(),
            secret: self.credential.secret.clone(),
            tool: name.to_string(),
            arguments,
        };
        match (self.transport)(&request) {
            Ok(wire::Response::Ok { result }) => {
                let text = serde_json::to_string_pretty(&result).unwrap_or_default();
                json!({
                    "content": [{ "type": "text", "text": text }],
                    "structuredContent": result,
                    "isError": false,
                })
            }
            Ok(wire::Response::Error { message }) => tool_error(&message),
            Ok(wire::Response::Unauthorized) => tool_error(REVOKED),
            Err(CallError::NotRunning) => tool_error(NOT_RUNNING),
            Err(CallError::Failed(message)) => tool_error(&message),
        }
    }
}

fn tool_error(message: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

fn result_reply(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn send(
    dir: &Path,
    credential: &Path,
    request: &wire::Request,
) -> Result<wire::Response, CallError> {
    let endpoint: Endpoint = std::fs::read(dir.join("endpoint.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or(CallError::NotRunning)?;
    if endpoint.v != wire::PROTOCOL {
        return Err(CallError::Failed(
            "TagRex was updated since the assistant started; restart the assistant".into(),
        ));
    }
    let mut stream = connect(&endpoint, dir, credential)?;
    let body = serde_json::to_vec(request).map_err(|e| CallError::Failed(e.to_string()))?;
    wire::write_frame(&mut stream, &body).map_err(|_| CallError::NotRunning)?;
    let answer = wire::read_frame(&mut stream, wire::MAX_RESPONSE)
        .map_err(|error| CallError::Failed(format!("TagRex did not answer: {error}")))?;
    serde_json::from_slice(&answer).map_err(|error| CallError::Failed(error.to_string()))
}

#[cfg(unix)]
fn connect(
    endpoint: &Endpoint,
    dir: &Path,
    credential: &Path,
) -> Result<std::os::unix::net::UnixStream, CallError> {
    use std::os::unix::fs::MetadataExt;
    if endpoint.transport != "unix" {
        return Err(CallError::NotRunning);
    }
    // The socket must be the one TagRex made for this user, in its own folder:
    // owned by whoever owns the credential, so another account cannot stand in
    // for the window and collect the secret.
    let address = Path::new(&endpoint.address);
    let owner = std::fs::metadata(credential)
        .map_err(|e| CallError::Failed(e.to_string()))?
        .uid();
    let socket_owner = std::fs::symlink_metadata(address)
        .map_err(|_| CallError::NotRunning)?
        .uid();
    if address.parent() != Some(dir) || socket_owner != owner {
        return Err(CallError::Failed(
            "the TagRex endpoint is not where TagRex puts it; refusing to connect".into(),
        ));
    }
    let stream =
        std::os::unix::net::UnixStream::connect(address).map_err(|_| CallError::NotRunning)?;
    let _ = stream.set_read_timeout(Some(CALL_TIMEOUT));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    Ok(stream)
}

#[cfg(windows)]
fn connect(
    endpoint: &Endpoint,
    _dir: &Path,
    _credential: &Path,
) -> Result<std::fs::File, CallError> {
    if endpoint.transport != "pipe" || !endpoint.address.starts_with(r"\\.\pipe\tagrex-mcp-") {
        return Err(CallError::Failed(
            "the TagRex endpoint is not one TagRex makes; refusing to connect".into(),
        ));
    }
    // All instances busy: the window makes the next one as soon as it takes a
    // call, so wait a little.
    const ERROR_PIPE_BUSY: i32 = 231;
    for _ in 0..50 {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&endpoint.address)
        {
            Ok(file) => return Ok(file),
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return Err(CallError::NotRunning),
        }
    }
    Err(CallError::Failed("TagRex is busy; try again".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn connector(
        answer: Result<wire::Response, CallError>,
    ) -> (Connector, Rc<RefCell<Vec<String>>>) {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let answer = RefCell::new(Some(answer));
        let connector = Connector {
            credential: Credential {
                v: 1,
                client: "ab12".into(),
                secret: "s3cret".into(),
            },
            transport: Box::new(move |request| {
                assert_eq!(request.client, "ab12");
                assert_eq!(request.secret, "s3cret");
                log.borrow_mut().push(request.tool.clone());
                answer.borrow_mut().take().expect("one call")
            }),
        };
        (connector, seen)
    }

    fn reply(connector: &Connector, line: &str) -> Value {
        connector.handle_line(line).expect("a reply")
    }

    #[test]
    fn initialize_echoes_a_known_version_and_offers_tools() {
        let (c, _) = connector(Err(CallError::NotRunning));
        let r = reply(
            &c,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
        );
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        assert_eq!(r["result"]["serverInfo"]["name"], "tagrex");
    }

    #[test]
    fn an_unknown_version_is_answered_with_the_newest() {
        let (c, _) = connector(Err(CallError::NotRunning));
        let r = reply(
            &c,
            r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
        );
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn notifications_get_no_answer() {
        let (c, _) = connector(Err(CallError::NotRunning));
        assert!(c
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none());
    }

    #[test]
    fn protocol_errors_use_the_json_rpc_codes() {
        let (c, _) = connector(Err(CallError::NotRunning));
        assert_eq!(reply(&c, "{not json")["error"]["code"], -32700);
        assert_eq!(reply(&c, "[]")["error"]["code"], -32600);
        let r = reply(&c, r#"{"jsonrpc":"2.0","id":2,"method":"resources/list"}"#);
        assert_eq!(r["error"]["code"], -32601);
        let r = reply(
            &c,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"write_tags"}}"#,
        );
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn tools_list_needs_no_window() {
        let (c, seen) = connector(Err(CallError::NotRunning));
        let r = reply(&c, r#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#);
        assert!(r["result"]["tools"].as_array().unwrap().len() >= 6);
        assert!(seen.borrow().is_empty());
    }

    #[test]
    fn a_tool_result_comes_back_as_text_and_structured_content() {
        let (c, seen) = connector(Ok(wire::Response::Ok {
            result: json!({ "open": false }),
        }));
        let r = reply(
            &c,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"library_status","arguments":{}}}"#,
        );
        assert_eq!(r["result"]["isError"], false);
        assert_eq!(r["result"]["structuredContent"]["open"], false);
        assert!(r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"open\": false"));
        assert_eq!(*seen.borrow(), vec!["library_status".to_string()]);
    }

    #[test]
    fn a_closed_window_or_a_revoked_client_is_a_tool_error_not_a_crash() {
        let call =
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"list_fields"}}"#;
        let (c, _) = connector(Err(CallError::NotRunning));
        let r = reply(&c, call);
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(r["result"]["content"][0]["text"], NOT_RUNNING);
        let (c, _) = connector(Ok(wire::Response::Unauthorized));
        assert_eq!(reply(&c, call)["result"]["content"][0]["text"], REVOKED);
    }

    #[test]
    fn the_credential_flag_is_found_among_the_arguments() {
        let args = ["tagrex", "--mcp-stdio", "--mcp-credential", "/x/c.json"]
            .into_iter()
            .map(std::ffi::OsString::from);
        assert_eq!(credential_arg(args), Some(PathBuf::from("/x/c.json")));
        let args = ["tagrex", "--mcp-stdio"]
            .into_iter()
            .map(std::ffi::OsString::from);
        assert_eq!(credential_arg(args), None);
    }
}
