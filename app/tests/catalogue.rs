//! Every code the backend can emit has to exist in every interface catalogue
//! (#268).
//!
//! The two sides cannot see each other: the codes are Rust string literals, the
//! catalogues are ES modules with no build step to check them against. A code
//! with no entry falls back to English, which is the designed behaviour for a
//! *newer backend talking to an older frontend* — but inside one build it just
//! means someone added a message and forgot half of it, and the fallback would
//! hide that until a user on a translated language hit the error.
//!
//! So this reads the sources as text. Crude, deliberately: the alternative is a
//! hand-kept list of codes, which is one more thing to forget to update.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The command layer's source, read as text rather than linked: the codes are
/// string literals, and there is nothing to call that would list them.
fn command_layer_source() -> PathBuf {
    crate_dir()
        .parent()
        .expect("the app crate has a parent directory")
        .join("crates/commands/src/lib.rs")
}

/// Every catalogue in the directory, by its language code — read rather than
/// listed, so adding a language does not also mean remembering to add it here
/// (#269). English is left out: it is the side the others are checked against.
fn other_catalogues() -> Vec<(String, PathBuf)> {
    let dir = crate_dir().join("ui/js/i18n");
    let mut found: Vec<_> = std::fs::read_dir(&dir)
        .expect("read the catalogue directory")
        .map(|entry| entry.expect("read a catalogue entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "js"))
        .filter_map(|path| {
            let code = path.file_stem()?.to_str()?.to_string();
            (code != "en").then_some((code, path))
        })
        .collect();
    found.sort();
    assert!(!found.is_empty(), "no catalogue besides en.js in {dir:?}");
    found
}

/// Every `"error.…"` / `"plan.…"` literal in the command layer.
///
/// The test stays in this crate because the catalogues it checks against live
/// under `ui/`, but the layer it scans moved out to `crates/commands` (#272),
/// so the path steps out of the manifest directory to reach it.
fn codes_in_rust() -> BTreeSet<String> {
    let source = std::fs::read_to_string(command_layer_source()).expect("read lib.rs");
    let mut codes = BTreeSet::new();
    for piece in source.split('"').skip(1).step_by(2) {
        if (piece.starts_with("error.") || piece.starts_with("plan."))
            && piece
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
        {
            codes.insert(piece.to_string());
        }
    }
    assert!(codes.len() > 50, "the scan found almost nothing: {codes:?}");
    codes
}

/// The keys a catalogue defines, by their `"key":` line.
fn keys_in_catalogue(path: &Path) -> BTreeSet<String> {
    let source = std::fs::read_to_string(path).expect("read catalogue");
    let mut keys = BTreeSet::new();
    for line in source.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix('"') else {
            continue;
        };
        let Some((key, after)) = rest.split_once('"') else {
            continue;
        };
        if after.starts_with(':') {
            keys.insert(key.to_string());
        }
    }
    keys
}

/// The placeholder reference builds its codes with `format!`, so the text scan
/// cannot see them — it asks the function instead, which is the stronger check
/// of the two anyway: these are the codes that will actually be sent.
fn placeholder_codes() -> BTreeSet<String> {
    let mut codes = BTreeSet::new();
    for entry in tagrex_commands::mask_placeholders() {
        codes.insert(entry.code);
        codes.insert(entry.group_code);
    }
    assert!(codes.len() > 40, "the reference looks empty: {codes:?}");
    codes
}

#[test]
fn every_backend_code_is_in_every_catalogue() {
    let mut codes = codes_in_rust();
    codes.extend(placeholder_codes());
    let mut catalogues = other_catalogues();
    catalogues.push(("en".to_string(), crate_dir().join("ui/js/i18n/en.js")));
    for (language, path) in catalogues {
        let keys = keys_in_catalogue(&path);
        let missing: Vec<_> = codes.difference(&keys).collect();
        assert!(
            missing.is_empty(),
            "{language}.js has no entry for: {missing:?}"
        );
    }
}

/// The other direction, every translation against English: a key in one and
/// not the other is a half-finished translation, and English falling back
/// silently is exactly what makes it easy to miss.
#[test]
fn the_catalogues_hold_the_same_keys() {
    let en = keys_in_catalogue(&crate_dir().join("ui/js/i18n/en.js"));
    for (language, path) in other_catalogues() {
        let keys = keys_in_catalogue(&path);
        let untranslated: Vec<_> = en.difference(&keys).collect();
        let unknown: Vec<_> = keys.difference(&en).collect();
        assert!(
            untranslated.is_empty(),
            "not in {language}.js: {untranslated:?}"
        );
        assert!(
            unknown.is_empty(),
            "in {language}.js but not in en.js: {unknown:?}"
        );
    }
}
