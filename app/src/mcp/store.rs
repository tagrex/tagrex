//! Paired assistants (#828).
//!
//! Pairing makes a random secret, writes it to a credential file only the user
//! can read, and keeps just its SHA-256 in `clients.json`. The assistant's MCP
//! configuration names the credential file, never the secret, so the setup
//! command can be shown again at any time without exposing it. Revoking removes
//! both.
//!
//! Layout, inside the app's settings folder:
//!
//! ```text
//! mcp/                      owner-only
//!   clients.json            paired clients, secrets hashed
//!   endpoint.json           where the running window listens
//!   credentials/<id>.json   one client's secret
//! ```

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The assistants a pairing can be made for.
pub const KINDS: [&str; 2] = ["claude-code", "claude-desktop"];
/// `last_used` is written at most this often, so a busy assistant does not
/// rewrite the file on every call.
const LAST_USED_GRANULARITY_SECS: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Client {
    pub id: String,
    pub kind: String,
    /// Seconds since the Unix epoch.
    pub created: u64,
    #[serde(default)]
    pub last_used: Option<u64>,
    pub secret_sha256: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ClientsFile {
    v: u32,
    clients: Vec<Client>,
}

/// What the connector reads to prove who it is.
#[derive(Debug, Serialize, Deserialize)]
pub struct Credential {
    pub v: u32,
    pub client: String,
    pub secret: String,
}

pub struct Store {
    dir: PathBuf,
    // Pairing and revoking come from the interface while the listener checks
    // calls; one at a time keeps clients.json whole.
    lock: Mutex<()>,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            lock: Mutex::new(()),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn endpoint_path(&self) -> PathBuf {
        self.dir.join("endpoint.json")
    }

    pub fn credential_path(&self, id: &str) -> PathBuf {
        self.dir.join("credentials").join(format!("{id}.json"))
    }

    fn clients_path(&self) -> PathBuf {
        self.dir.join("clients.json")
    }

    pub fn clients(&self) -> io::Result<Vec<Client>> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.load()
    }

    /// Pair a new client of `kind`; returns it and its credential file.
    pub fn pair(&self, kind: &str) -> io::Result<(Client, PathBuf)> {
        if !KINDS.contains(&kind) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown assistant: {kind}"),
            ));
        }
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut clients = self.load()?;
        let id = random_hex(8)?;
        let secret = random_hex(32)?;
        let credential = self.credential_path(&id);
        ensure_private_dir(&self.dir)?;
        ensure_private_dir(credential.parent().expect("credentials folder"))?;
        let body = serde_json::to_vec_pretty(&Credential {
            v: 1,
            client: id.clone(),
            secret: secret.clone(),
        })?;
        write_private(&credential, &body)?;
        let client = Client {
            id,
            kind: kind.to_string(),
            created: now(),
            last_used: None,
            secret_sha256: sha256_hex(&secret),
        };
        clients.push(client.clone());
        self.save(&clients)?;
        Ok((client, credential))
    }

    /// Forget a client and delete its credential file. `false` when there was
    /// no such client.
    pub fn revoke(&self, id: &str) -> io::Result<bool> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut clients = self.load()?;
        let before = clients.len();
        clients.retain(|client| client.id != id);
        let found = clients.len() != before;
        if found {
            self.save(&clients)?;
        }
        // Gone from the list is what revokes it; the file is only tidied.
        if is_client_id(id) {
            match std::fs::remove_file(self.credential_path(id)) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
        Ok(found)
    }

    /// Whether `secret` is the one paired as `id`, noting the time when it is.
    pub fn authenticate(&self, id: &str, secret: &str) -> io::Result<bool> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut clients = self.load()?;
        let offered = sha256_hex(secret);
        let Some(client) = clients.iter_mut().find(|client| client.id == id) else {
            return Ok(false);
        };
        if !constant_time_eq(client.secret_sha256.as_bytes(), offered.as_bytes()) {
            return Ok(false);
        }
        let now = now();
        let stale = client
            .last_used
            .is_none_or(|last| now.saturating_sub(last) >= LAST_USED_GRANULARITY_SECS);
        if stale {
            client.last_used = Some(now);
            // A failure to note the time must not refuse a valid client.
            let _ = self.save(&clients);
        }
        Ok(true)
    }

    fn load(&self) -> io::Result<Vec<Client>> {
        match std::fs::read(self.clients_path()) {
            Ok(bytes) => {
                let file: ClientsFile = serde_json::from_slice(&bytes)?;
                Ok(file.clients)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    fn save(&self, clients: &[Client]) -> io::Result<()> {
        ensure_private_dir(&self.dir)?;
        let body = serde_json::to_vec_pretty(&ClientsFile {
            v: 1,
            clients: clients.to_vec(),
        })?;
        write_private(&self.clients_path(), &body)
    }
}

/// An id this module made: hex only, so it is safe in a file name.
fn is_client_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

pub fn random_hex(bytes: usize) -> io::Result<String> {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(hex(&buffer))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Create `dir` if needed and keep it to its owner.
pub fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write `body` to `path` through a synced temporary file, readable by its
/// owner only, so a crash never leaves half a file behind.
pub fn write_private(path: &Path, body: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temp = path.with_file_name(format!(".{name}.tmp"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(body)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempdir::Dir, Store) {
        let dir = tempdir::Dir::new();
        let store = Store::new(dir.path().join("mcp"));
        (dir, store)
    }

    // A throwaway folder without another dependency.
    mod tempdir {
        pub struct Dir(std::path::PathBuf);
        impl Dir {
            pub fn new() -> Self {
                let path = std::env::temp_dir()
                    .join(format!("tagrex-mcp-{}", super::random_hex(8).unwrap()));
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }
            pub fn path(&self) -> &std::path::Path {
                &self.0
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn read_credential(path: &Path) -> Credential {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn a_paired_client_authenticates_with_its_secret_only() {
        let (_dir, store) = store();
        let (client, path) = store.pair("claude-code").unwrap();
        let credential = read_credential(&path);
        assert_eq!(credential.client, client.id);
        assert!(store.authenticate(&client.id, &credential.secret).unwrap());
        assert!(!store.authenticate(&client.id, "not the secret").unwrap());
        assert!(!store.authenticate("0000", &credential.secret).unwrap());
    }

    #[test]
    fn the_secret_is_not_kept_in_the_client_list() {
        let (_dir, store) = store();
        let (_client, path) = store.pair("claude-desktop").unwrap();
        let secret = read_credential(&path).secret;
        let list = std::fs::read_to_string(store.dir().join("clients.json")).unwrap();
        assert!(!list.contains(&secret));
        assert!(list.contains(&sha256_hex(&secret)));
    }

    #[test]
    fn a_revoked_client_is_refused_and_its_credential_removed() {
        let (_dir, store) = store();
        let (client, path) = store.pair("claude-code").unwrap();
        let secret = read_credential(&path).secret;
        assert!(store.revoke(&client.id).unwrap());
        assert!(!path.exists());
        assert!(!store.authenticate(&client.id, &secret).unwrap());
        assert!(!store.revoke(&client.id).unwrap());
    }

    #[test]
    fn use_is_noted_on_the_client() {
        let (_dir, store) = store();
        let (client, path) = store.pair("claude-code").unwrap();
        assert_eq!(store.clients().unwrap()[0].last_used, None);
        store
            .authenticate(&client.id, &read_credential(&path).secret)
            .unwrap();
        assert!(store.clients().unwrap()[0].last_used.is_some());
    }

    #[test]
    fn an_unknown_kind_is_not_paired() {
        let (_dir, store) = store();
        assert!(store.pair("something-else").is_err());
        assert!(store.clients().unwrap().is_empty());
    }

    #[test]
    fn a_revoke_never_reaches_outside_the_credentials_folder() {
        let (dir, store) = store();
        let outside = dir.path().join("keep.json");
        std::fs::write(&outside, b"{}").unwrap();
        assert!(!store.revoke("../../keep").unwrap());
        assert!(outside.exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_credential_and_its_folder_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, store) = store();
        let (_client, path) = store.pair("claude-code").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(store.dir()), 0o700);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }
}
