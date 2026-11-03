//! Audit harness: round-trips edge-case values through TagEngine on real
//! (synthetic) files and reports every deviation instead of stopping at the first.
//! Ignored by default. Run: cargo test -p tagrex-core --test audit_roundtrip -- --nocapture

use std::collections::BTreeMap;
use std::path::PathBuf;

use tagrex_core::model::{AudioFormat, TagEngine, TagField, TrackFile};

/// fLaC + STREAMINFO (not last) + 16-byte PADDING (last) + one silent frame.
fn flac_bytes() -> Vec<u8> {
    let mut v = vec![0x66, 0x4c, 0x61, 0x43, 0x00, 0x00, 0x00, 0x22];
    v.extend_from_slice(&[0x10, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    v.extend_from_slice(&[0x0a, 0xc4, 0x42, 0xf0, 0x00, 0x00, 0x00, 0x00]);
    v.extend_from_slice(&[0u8; 16]);
    v.extend_from_slice(&[0x81, 0x00, 0x00, 0x10]);
    v.extend_from_slice(&[0u8; 16]);
    v.extend_from_slice(&[
        0xff, 0xf8, 0xc9, 0x18, 0x00, 0xc2, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xb8, 0xee,
    ]);
    v
}
const MINIMAL_M4A: &[u8] = include_bytes!("fixtures/minimal.m4a");

/// 20 MPEG-1 Layer III frames, 128 kbps, 44.1 kHz, stereo (417 bytes each).
fn mp3_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..20 {
        let mut frame = vec![0u8; 417];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        out.extend_from_slice(&frame);
    }
    out
}

fn tmp(name: &str, ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!("tagrex-audit-{name}-{}.{ext}", std::process::id()))
}

fn field_values() -> Vec<(&'static str, TagField, String)> {
    let mut v: Vec<(&'static str, TagField, String)> = Vec::new();
    let text = [
        ("unicode", "Ünïcödé 日本語 🎵 Ω"),
        ("long", &"x".repeat(10_000)),
        ("newline", "line1\nline2"),
        ("crlf", "a\r\nb"),
        ("nul", "a\0b"),
        ("semi", "A; B"),
        ("slash", "A/B"),
        ("backslash", "A\\B"),
        ("padded", " x "),
        ("spaces-only", "   "),
        ("tab", "a\tb"),
        ("equals", "a=b"),
        ("bom", "\u{feff}bom"),
        ("quote", "\"q\" 'q'"),
        ("combining", "e\u{0301}"),
        ("rtl", "שלום"),
        ("ctrl", "a\u{7}b"),
    ];
    for (n, t) in text {
        for f in [
            TagField::Artist,
            TagField::Title,
            TagField::Album,
            TagField::Comment,
            TagField::Genre,
        ] {
            v.push((n, f, t.to_string()));
        }
    }
    for t in [
        "07",
        "1/12",
        "A1",
        "0",
        "-1",
        "999999999999",
        "1.5",
        " 3",
        "3 ",
        "１２",
    ] {
        v.push(("tracknum", TagField::TrackNumber, t.to_string()));
        v.push(("discnum", TagField::DiscNumber, t.to_string()));
        v.push(("tracktotal", TagField::TrackTotal, t.to_string()));
    }
    for t in [
        "1996",
        "1996-05-01",
        "0000",
        "9999",
        "1996-13-45",
        "1996-5",
        "1996/05",
    ] {
        v.push(("year", TagField::Year, t.to_string()));
    }
    for t in ["128", "128.5", "abc", "-5", "0", "99999999999", "128,5", ""] {
        v.push(("bpm", TagField::Bpm, t.to_string()));
    }
    for t in ["Am", "12A", "H#", "", "zz"] {
        v.push(("key", TagField::InitialKey, t.to_string()));
    }
    for t in ["GBAYE0601498", "gbaye0601498", "GB-AYE-06-01498", "x"] {
        v.push(("isrc", TagField::Isrc, t.to_string()));
    }
    v.push(("url", TagField::Url, "https://example.com/a b?x=é".into()));
    v.push(("url", TagField::Url, "not a url".into()));
    for name in [
        "lower",
        "Spaces In Name",
        "a=b",
        "",
        "ÜNI",
        "A".repeat(300).leak(),
        "TXXX",
        "COMM",
        "title",
        "TITLE",
        "ARTIST",
        "\u{1F3B5}",
    ] {
        v.push(("custom", TagField::Custom(name.to_string()), "value".into()));
        v.push((
            "custom-uni",
            TagField::Custom(name.to_string()),
            "vál ü 日".into(),
        ));
    }
    v
}

fn run_format(label: &str, ext: &str, format: AudioFormat, base: Vec<u8>, audio_tail: usize) {
    let mut deviations: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut total = 0usize;

    for (case, field, value) in field_values() {
        total += 1;
        let path = tmp(&format!("{label}-{total}"), ext);
        std::fs::write(&path, &base).unwrap();
        let before = std::fs::read(&path).unwrap();

        let mut tags = BTreeMap::new();
        tags.insert(field.clone(), value.clone());
        let res = std::panic::catch_unwind(|| {
            TagEngine::write(&TrackFile {
                path: path.clone(),
                format,
                tags,
            })
        });
        let desc = format!("[{label}] {case} {field:?} = {:?}", short(&value));
        match res {
            Err(_) => deviations.push(format!("PANIC on write: {desc}")),
            Ok(Err(e)) => {
                errors.push(format!("{desc} -> write refused: {e}"));
                let after = std::fs::read(&path).unwrap();
                if after != before {
                    deviations.push(format!("REFUSED BUT FILE CHANGED: {desc}"));
                }
            }
            Ok(Ok(())) => {
                let after = std::fs::read(&path).unwrap();
                if audio_tail > 0
                    && (after.len() < audio_tail
                        || after[after.len() - audio_tail..] != before[before.len() - audio_tail..])
                {
                    deviations.push(format!("AUDIO TAIL ALTERED: {desc}"));
                }
                match std::panic::catch_unwind(|| TagEngine::read(&path)) {
                    Err(_) => deviations.push(format!("PANIC on read-back: {desc}")),
                    Ok(Err(e)) => deviations.push(format!("UNREADABLE AFTER WRITE ({e}): {desc}")),
                    Ok(Ok(read)) => {
                        let got = read.tags.get(&field).cloned();
                        let want = if value.is_empty() {
                            None
                        } else {
                            Some(value.clone())
                        };
                        if got != want && !(value.is_empty() && got.as_deref() == Some("")) {
                            deviations.push(format!(
                                "VALUE CHANGED: {desc} -> {:?}",
                                got.as_deref().map(short)
                            ));
                        }
                        // second write of the read-back map must be a fixed point
                        let again = TagEngine::write(&read);
                        if let Err(e) = again {
                            deviations.push(format!("REWRITE OF READ-BACK FAILED ({e}): {desc}"));
                        } else if let Ok(r2) = TagEngine::read(&path) {
                            if r2.tags != read.tags {
                                deviations.push(format!("NOT IDEMPOTENT: {desc}"));
                            }
                        }
                    }
                }
            }
        }
        std::fs::remove_file(&path).ok();
    }

    println!(
        "\n===== {label}: {total} cases, {} deviations, {} refusals =====",
        deviations.len(),
        errors.len()
    );
    for d in &deviations {
        println!("DEV  {d}");
    }
    for e in &errors {
        println!("REF  {e}");
    }
}

fn short(s: &str) -> String {
    if s.chars().count() > 40 {
        format!(
            "{}…(len {})",
            s.chars().take(40).collect::<String>(),
            s.chars().count()
        )
    } else {
        s.to_string()
    }
}

#[test]
#[ignore = "audit harness: prints findings, run with --ignored --nocapture"]
fn audit_flac() {
    run_format("flac", "flac", AudioFormat::Flac, flac_bytes(), 14);
}

#[test]
#[ignore = "audit harness: prints findings, run with --ignored --nocapture"]
fn audit_mp3() {
    run_format("mp3", "mp3", AudioFormat::Mp3, mp3_bytes(), 417 * 20);
}

#[test]
#[ignore = "audit harness: prints findings, run with --ignored --nocapture"]
fn audit_m4a() {
    run_format("m4a", "m4a", AudioFormat::M4a, MINIMAL_M4A.to_vec(), 0);
}
