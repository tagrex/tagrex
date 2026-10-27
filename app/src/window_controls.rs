//! Which end of an in-app window's head the close button goes at (#565).
//!
//! The frontend decides macOS and Windows from the platform alone. On Linux the
//! desktop's own button layout is a user setting, and an in-app window that
//! ignored a close-on-the-left layout would be the one exception on that
//! desktop, so it is read here: GNOME (and its derivatives) through
//! `gsettings`, KDE from `kwinrc`. Anything that can't be read leaves the
//! default, the end.

/// Whether a GNOME `button-layout` value (`'appmenu:minimize,maximize,close'`,
/// `close,minimize,maximize:`) puts close on the left: before the colon. `None`
/// when the value has no colon or no close at all.
// Only called on Linux; the tests run everywhere.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn gnome_close_on_left(layout: &str) -> Option<bool> {
    let layout = layout.trim().trim_matches(|c| c == '\'' || c == '"');
    let (left, right) = layout.split_once(':')?;
    let has_close = |side: &str| side.split(',').any(|button| button.trim() == "close");
    if has_close(left) {
        Some(true)
    } else if has_close(right) {
        Some(false)
    } else {
        None
    }
}

/// Whether a `kwinrc` puts close on the left: `ButtonsOnLeft` in the
/// `[org.kde.kdecoration2]` group holds `X`. `None` when the key is not set,
/// which leaves KDE's default (close on the right).
// Only called on Linux; the tests run everywhere.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn kde_close_on_left(kwinrc: &str) -> Option<bool> {
    let mut in_group = false;
    for line in kwinrc.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == "[org.kde.kdecoration2]";
        } else if in_group {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "ButtonsOnLeft" {
                    return Some(value.contains('X'));
                }
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn close_on_left() -> Option<bool> {
    use std::process::Command;

    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if desktop.to_ascii_uppercase().contains("KDE") {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".config"))
            })?;
        let kwinrc = std::fs::read_to_string(config.join("kwinrc")).ok()?;
        return kde_close_on_left(&kwinrc);
    }
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.wm.preferences", "button-layout"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    gnome_close_on_left(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "linux"))]
fn close_on_left() -> Option<bool> {
    None
}

/// `"start"` when the desktop puts close on the left, `"end"` otherwise.
pub fn close_button_side() -> &'static str {
    if close_on_left() == Some(true) {
        "start"
    } else {
        "end"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_gnome_button_layout() {
        assert_eq!(
            gnome_close_on_left("'appmenu:minimize,maximize,close'\n"),
            Some(false)
        );
        assert_eq!(gnome_close_on_left("'appmenu:close'"), Some(false));
        assert_eq!(
            gnome_close_on_left("'close,minimize,maximize:'"),
            Some(true)
        );
        assert_eq!(gnome_close_on_left("close:appmenu"), Some(true));
        assert_eq!(gnome_close_on_left("'appmenu:'"), None);
        assert_eq!(gnome_close_on_left("'closeish'"), None);
    }

    #[test]
    fn reads_the_kde_buttons_on_left() {
        let left =
            "[Windows]\nFoo=1\n\n[org.kde.kdecoration2]\nButtonsOnLeft=XIA\nButtonsOnRight=\n";
        assert_eq!(kde_close_on_left(left), Some(true));
        let right = "[org.kde.kdecoration2]\nButtonsOnLeft=MS\nButtonsOnRight=HIAX\n";
        assert_eq!(kde_close_on_left(right), Some(false));
        // the key in another group, or not set at all: KDE's default
        assert_eq!(kde_close_on_left("[Other]\nButtonsOnLeft=X\n"), None);
        assert_eq!(kde_close_on_left(""), None);
    }
}
