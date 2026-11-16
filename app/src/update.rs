//! Telling the user that a newer TagRex exists (#435).
//!
//! One request to the GitHub Releases API for the latest published release,
//! compared with the running version. Nothing about the user or their files is
//! sent beyond what any HTTPS request carries (an address and a user agent
//! naming the version).
//!
//! When that release is newer, the updater plugin reads its `latest.json` too
//! (#602): if the release has a signed bundle for this platform and install
//! type, the notice can offer to install it. Nothing is downloaded until the
//! user asks for it.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Where the releases are, and the page the notice opens.
const LATEST_API: &str = "https://api.github.com/repos/tagrex/tagrex/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/tagrex/tagrex/releases/latest";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateCheckDto {
    /// The version this build is.
    pub current: String,
    /// The latest published release, without a leading `v`.
    pub latest: String,
    /// Whether `latest` is newer than `current`.
    pub newer: bool,
    /// Whether `latest` can be installed from inside the app (#602): its release
    /// has a signed update bundle for this platform and install type.
    pub installable: bool,
}

/// The update the last check found installable (#602), held for Install.
#[derive(Default)]
pub struct PendingUpdate(pub Mutex<Option<Update>>);

/// `0.21.0`, `v0.21.0` or `0.21.0-rc1` as numbers, the pre-release suffix
/// dropped. `None` for anything that does not start with a dotted number.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next().unwrap_or(core);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// Whether `latest` is a newer release than `current`. An answer either side of
/// which cannot be read is "no": a notice that might be wrong is worse than none.
fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// Ask GitHub for the latest release. Honours the proxy of the network settings.
pub fn check(proxy: &str) -> Result<UpdateCheckDto, String> {
    let current = env!("CARGO_PKG_VERSION");
    let mut builder = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)));
    if !proxy.trim().is_empty() {
        let proxy =
            ureq::Proxy::new(proxy.trim()).map_err(|error| format!("invalid proxy: {error}"))?;
        builder = builder.proxy(Some(proxy));
    }
    let agent = ureq::Agent::new_with_config(builder.build());
    let mut response = agent
        .get(LATEST_API)
        .header("User-Agent", &format!("TagRex/{current}"))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(format!("GitHub answered {status}"));
    }
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    let tag = json
        .get("tag_name")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "GitHub's answer has no release tag".to_string())?;
    let latest = tag.trim_start_matches('v').to_string();
    Ok(UpdateCheckDto {
        newer: is_newer(&latest, current),
        current: current.to_string(),
        latest,
        installable: false,
    })
}

/// The proxy of the network settings as the updater's client takes it: a URL,
/// `http://` assumed when the setting is a bare `host:port`.
fn proxy_url(proxy: &str) -> Option<Url> {
    let proxy = proxy.trim();
    if proxy.is_empty() {
        return None;
    }
    Url::parse(proxy)
        .ok()
        .filter(|url| url.has_host())
        .or_else(|| Url::parse(&format!("http://{proxy}")).ok())
}

/// The installable update for `latest`, read from its release's `latest.json`
/// (#602). `None` whenever that is not on offer, for whatever reason: a release
/// published before there were update bundles, no bundle for this target (a
/// build that was not installed from a package), a `latest.json` that names
/// another version, or no answer at all. The notice then stays what it was, a
/// way to the release page.
pub async fn installable(app: &AppHandle, proxy: &str, latest: &str) -> Option<Update> {
    let mut builder = app.updater_builder().timeout(Duration::from_secs(10));
    if let Some(url) = proxy_url(proxy) {
        builder = builder.proxy(url);
    }
    let update = builder.build().ok()?.check().await.ok()??;
    let same = parse_version(&update.version).is_some()
        && parse_version(&update.version) == parse_version(latest);
    same.then_some(update)
}

/// The folder the running app sits in, when its user cannot write there (#699).
///
/// The installer runs without administrator rights and keeps the folder it was
/// last installed to, so a copy put under `Program Files` by an installer run as
/// administrator stops every later update with "Error opening file for writing".
#[cfg(windows)]
pub fn protected_install_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    refuses_writes(dir).then(|| dir.to_path_buf())
}

/// Whether `dir` refuses this user a new file. Only a refusal counts: anything
/// else (a leftover probe, a full disk) is for the installer to find and report.
#[cfg(any(windows, test))]
fn refuses_writes(dir: &std::path::Path) -> bool {
    let probe = dir.join(format!(".tagrex-update-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            false
        }
        Err(error) => error.kind() == std::io::ErrorKind::PermissionDenied,
    }
}

/// What the elevated installer is told: what the updater plugin passes for the
/// passive install mode, and the folder to install to. Elevating with another
/// account's password would otherwise look up that account's folder. NSIS takes
/// `/D=` only as the last argument and unquoted, spaces included.
#[cfg(any(windows, test))]
fn elevated_installer_parameters(dir: &std::path::Path) -> String {
    format!("/P /UPDATE /R /ARGS /D={}", dir.display())
}

/// Start the installer in `bytes`, whose signature the download has checked, with
/// administrator rights, installing to `dir`. Windows asks the user first;
/// declining is an error and the app goes on. The caller quits on success so the
/// installer can replace it.
#[cfg(windows)]
pub fn run_installer_elevated(
    bytes: &[u8],
    version: &str,
    dir: &std::path::Path,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;

    if !bytes.starts_with(b"MZ") {
        return Err("the update is not an installer".to_string());
    }
    let temp = std::env::temp_dir().join(format!("TagRex-{version}-update-{}", std::process::id()));
    std::fs::create_dir_all(&temp).map_err(|error| error.to_string())?;
    let installer = temp.join(format!("TagRex_{version}-setup.exe"));
    std::fs::write(&installer, bytes).map_err(|error| error.to_string())?;

    let wide = |text: &std::ffi::OsStr| -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    };
    let file = wide(installer.as_os_str());
    let parameters = wide(std::ffi::OsStr::new(&elevated_installer_parameters(dir)));
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("runas"),
            PCWSTR(file.as_ptr()),
            PCWSTR(parameters.as_ptr()),
            PCWSTR::null(),
            SW_SHOW,
        )
    };
    // At most 32 is a failure, and 5 is what declining the prompt comes back as.
    match result.0 as isize {
        code if code > 32 => Ok(()),
        5 => Err(format!(
            "TagRex needs administrator rights to update in {}",
            dir.display()
        )),
        code => Err(format!("the installer could not start (error {code})")),
    }
}

/// Whether this copy was installed from an rpm package (#787).
#[cfg(target_os = "linux")]
pub fn installed_from_rpm() -> bool {
    matches!(
        tauri::utils::platform::bundle_type(),
        Some(tauri::utils::config::BundleType::Rpm)
    )
}

/// What an rpm update comes to when the app installs it itself (#787).
#[cfg(target_os = "linux")]
pub enum RpmInstall {
    Installed,
    /// There is no `pkexec` to ask for root with; the updater plugin's own
    /// install, with its password prompt and `sudo`, is left to try.
    NoPkexec,
}

/// The command that installs the package at `path`. rpm 6 refuses a package
/// without an OpenPGP signature, which ours do not carry; the download has
/// checked the bundle against the minisign key built into the app, so only that
/// check is skipped. rpm still verifies the package digests.
#[cfg(any(target_os = "linux", test))]
fn rpm_install_args(path: &std::path::Path) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = ["rpm", "-U", "--nosignature"]
        .iter()
        .map(Into::into)
        .collect();
    args.push(path.into());
    args
}

/// Why `pkexec rpm` failed, in words: pkexec's own codes for a dismissed or
/// failed password prompt, otherwise what rpm wrote, which the plugin's error
/// ("Failed to install package") leaves out.
#[cfg(any(target_os = "linux", test))]
fn rpm_failure(code: Option<i32>, stderr: &str) -> String {
    let said: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let said = said.join("; ");
    match code {
        Some(126) => "the password prompt was dismissed".to_string(),
        // pkexec's own failure: its message is about polkit, not rpm.
        Some(127) if said.is_empty() => "authentication failed".to_string(),
        Some(127) => said,
        _ if !said.is_empty() => format!("rpm: {said}"),
        Some(code) => format!("rpm stopped with code {code}"),
        None => "rpm was stopped".to_string(),
    }
}

/// Install the rpm package in `bytes`, whose signature the download has
/// checked, as root through polkit: one password prompt (#787). The package is
/// written to a folder only this user can open, so nobody else can swap it
/// before rpm reads it, and the folder is removed afterwards either way.
#[cfg(target_os = "linux")]
pub fn install_rpm(bytes: &[u8]) -> Result<RpmInstall, String> {
    use std::os::unix::fs::DirBuilderExt;

    if !bytes.starts_with(&[0xed, 0xab, 0xee, 0xdb]) {
        return Err("the update is not an rpm package".to_string());
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!("tagrex-update-{}-{stamp}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|error| error.to_string())?;
    let package = dir.join("TagRex-update.rpm");
    let result = std::fs::write(&package, bytes)
        .map_err(|error| error.to_string())
        .and_then(|()| {
            match std::process::Command::new("pkexec")
                .args(rpm_install_args(&package))
                .output()
            {
                Ok(output) if output.status.success() => Ok(RpmInstall::Installed),
                Ok(output) => Err(rpm_failure(
                    output.status.code(),
                    &String::from_utf8_lossy(&output.stderr),
                )),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(RpmInstall::NoPkexec)
                }
                Err(error) => Err(error.to_string()),
            }
        });
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[cfg(test)]
mod tests {
    use super::{
        elevated_installer_parameters, is_newer, parse_version, proxy_url, refuses_writes,
        rpm_failure, rpm_install_args,
    };

    #[test]
    fn an_rpm_update_skips_only_the_signature_check() {
        let args = rpm_install_args(std::path::Path::new("/tmp/tagrex update/TagRex.rpm"));
        assert_eq!(
            args,
            [
                "rpm",
                "-U",
                "--nosignature",
                "/tmp/tagrex update/TagRex.rpm"
            ]
            .map(std::ffi::OsString::from)
        );
    }

    #[test]
    fn a_failed_rpm_install_says_why() {
        assert_eq!(
            rpm_failure(Some(126), ""),
            "the password prompt was dismissed"
        );
        assert_eq!(rpm_failure(Some(127), "\n"), "authentication failed");
        assert_eq!(
            rpm_failure(
                Some(127),
                "Error executing command as another user: Not authorized\n"
            ),
            "Error executing command as another user: Not authorized"
        );
        assert_eq!(
            rpm_failure(
                Some(1),
                "\tpackage tag-rex-0:1.6.1-1.aarch64 is already installed\n\n"
            ),
            "rpm: package tag-rex-0:1.6.1-1.aarch64 is already installed"
        );
        assert_eq!(
            rpm_failure(Some(1), "error: one\nerror: two\n"),
            "rpm: error: one; error: two"
        );
        assert_eq!(rpm_failure(Some(2), ""), "rpm stopped with code 2");
        assert_eq!(rpm_failure(None, ""), "rpm was stopped");
    }

    #[test]
    fn versions_read_with_or_without_a_v_and_a_suffix() {
        assert_eq!(parse_version("0.21.0"), Some((0, 21, 0)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.21.0-rc1"), Some((0, 21, 0)));
        assert_eq!(parse_version("0.21"), Some((0, 21, 0)));
        assert_eq!(parse_version("nightly"), None);
    }

    #[test]
    fn newer_means_strictly_newer_by_number_not_by_text() {
        assert!(is_newer("0.21.0", "0.20.0"));
        assert!(is_newer("0.10.0", "0.9.5"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.20.0", "0.20.0"));
        assert!(!is_newer("0.19.9", "0.20.0"));
    }

    /// Reaches the real API, so it only runs when asked for:
    /// `cargo test -p tagrex update -- --ignored`.
    #[test]
    #[ignore]
    fn the_latest_release_can_be_read_from_github() {
        let found = super::check("").expect("GitHub answers");
        assert!(super::parse_version(&found.latest).is_some(), "{found:?}");
    }

    #[test]
    fn a_bare_proxy_is_read_as_an_http_one() {
        assert_eq!(proxy_url(""), None);
        assert_eq!(
            proxy_url("proxy.local:3128").map(|url| url.to_string()),
            Some("http://proxy.local:3128/".to_string())
        );
        assert_eq!(
            proxy_url("socks5://127.0.0.1:1080").map(|url| url.to_string()),
            Some("socks5://127.0.0.1:1080".to_string())
        );
    }

    #[test]
    fn the_elevated_installer_is_told_the_folder_last_and_unquoted() {
        assert_eq!(
            elevated_installer_parameters(std::path::Path::new("Program Files/TagRex")),
            "/P /UPDATE /R /ARGS /D=Program Files/TagRex"
        );
    }

    #[test]
    fn a_writable_folder_needs_no_elevation_and_keeps_no_probe() {
        let dir = std::env::temp_dir().join(format!("tagrex-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!refuses_writes(&dir));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_folder_is_not_called_a_refusal() {
        let dir = std::env::temp_dir().join("tagrex-probe-no-such-folder");
        assert!(!refuses_writes(&dir));
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_folder_is_a_refusal() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("tagrex-probe-ro-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        // Root ignores the mode, so the check only means something for a user.
        let probe_landed = std::fs::write(dir.join("root-check"), b"").is_ok();
        let refused = refuses_writes(&dir);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        if !probe_landed {
            assert!(refused);
        }
    }

    #[test]
    fn an_unreadable_version_never_announces_an_update() {
        assert!(!is_newer("nightly", "0.20.0"));
        assert!(!is_newer("0.21.0", "dev"));
    }
}
