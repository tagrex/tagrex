//! Every code the backend can emit has to exist in both interface catalogues
//! (#268).
//!
//! The two sides cannot see each other: the codes are Rust string literals, the
//! catalogues are ES modules with no build step to check them against. A code
//! with no entry falls back to English, which is the designed behaviour for a
//! *newer backend talking to an older frontend* — but inside one build it just
//! means someone added a message and forgot half of it, and the fallback would
//! hide that until a Russian user hit the error.
//!
//! So this reads the sources as text. Crude, deliberately: the alternative is a
//! hand-kept list of codes, which is one more thing to forget to update.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `"error.…"` / `"plan.…"` literal in the command layer.
fn codes_in_rust() -> BTreeSet<String> {
    let source = std::fs::read_to_string(crate_dir().join("src/lib.rs")).expect("read lib.rs");
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
    for entry in tagrex::mask_placeholders() {
        codes.insert(entry.code);
        codes.insert(entry.group_code);
    }
    assert!(codes.len() > 40, "the reference looks empty: {codes:?}");
    codes
}

#[test]
fn every_backend_code_is_in_both_catalogues() {
    let mut codes = codes_in_rust();
    codes.extend(placeholder_codes());
    for language in ["en", "ru"] {
        let path = crate_dir().join(format!("ui/js/i18n/{language}.js"));
        let keys = keys_in_catalogue(&path);
        let missing: Vec<_> = codes.difference(&keys).collect();
        assert!(
            missing.is_empty(),
            "{language}.js has no entry for: {missing:?}"
        );
    }
}

/// The other direction, for the two catalogues against each other: a key in one
/// and not the other is a half-finished translation, and English falling back
/// silently is exactly what makes it easy to miss.
#[test]
fn the_catalogues_hold_the_same_keys() {
    let en = keys_in_catalogue(&crate_dir().join("ui/js/i18n/en.js"));
    let ru = keys_in_catalogue(&crate_dir().join("ui/js/i18n/ru.js"));
    let only_en: Vec<_> = en.difference(&ru).collect();
    let only_ru: Vec<_> = ru.difference(&en).collect();
    assert!(
        only_en.is_empty(),
        "not translated into Russian: {only_en:?}"
    );
    assert!(only_ru.is_empty(), "in ru.js but not in en.js: {only_ru:?}");
}
