//! Audit harness: ID3 text in the encodings real files carry (Latin-1 frames
//! holding cp1251 bytes, UTF-16 with and without a BOM, UTF-16BE, UTF-8, an
//! ID3v1 tag in cp1251), read and then saved after an edit of another field.
//! Ignored by default. Run: cargo test -p tagrex-core --test audit_encodings -- --ignored --nocapture

use std::path::PathBuf;

use tagrex_core::model::{TagEngine, TagField};

fn mp3_frames() -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..20 {
        let mut frame = vec![0u8; 417];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        out.extend_from_slice(&frame);
    }
    out
}

fn syncsafe(n: u32) -> [u8; 4] {
    [
        ((n >> 21) & 0x7f) as u8,
        ((n >> 14) & 0x7f) as u8,
        ((n >> 7) & 0x7f) as u8,
        (n & 0x7f) as u8,
    ]
}

/// "Кино" in cp1251.
const KINO_1251: &[u8] = &[0xCA, 0xE8, 0xED, 0xEE];

fn utf16le_bom(s: &str) -> Vec<u8> {
    let mut v = vec![0xFF, 0xFE];
    for u in s.encode_utf16() {
        v.extend_from_slice(&u.to_le_bytes());
    }
    v
}
fn utf16be_bom(s: &str) -> Vec<u8> {
    let mut v = vec![0xFE, 0xFF];
    for u in s.encode_utf16() {
        v.extend_from_slice(&u.to_be_bytes());
    }
    v
}
fn utf16be(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
}

fn frame(version: u8, id: &str, encoding: u8, text: &[u8]) -> Vec<u8> {
    let mut data = vec![encoding];
    data.extend_from_slice(text);
    let mut f = id.as_bytes().to_vec();
    if version == 4 {
        f.extend_from_slice(&syncsafe(data.len() as u32));
    } else {
        f.extend_from_slice(&(data.len() as u32).to_be_bytes());
    }
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(&data);
    f
}

fn tag(version: u8, frames: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = frames.concat();
    let mut t = b"ID3".to_vec();
    t.extend_from_slice(&[version, 0, 0]);
    t.extend_from_slice(&syncsafe(body.len() as u32));
    t.extend_from_slice(&body);
    t
}

fn id3v1(title: &[u8], artist: &[u8]) -> Vec<u8> {
    let mut t = b"TAG".to_vec();
    let mut field = |bytes: &[u8], len: usize| {
        let mut f = bytes.to_vec();
        f.resize(len, 0);
        t.extend_from_slice(&f);
    };
    field(title, 30);
    field(artist, 30);
    field(b"", 30);
    field(b"1990", 4);
    field(b"", 30);
    t.push(17); // Rock
    t
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("tagrex-enc-{name}-{}.mp3", std::process::id()))
}

fn find(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn case(name: &str, bytes: Vec<u8>, probes: &[(&str, &[u8])]) {
    let path = tmp(name);
    std::fs::write(&path, &bytes).unwrap();
    let before = TagEngine::read(&path);
    println!("\n== {name}");
    match &before {
        Ok(t) => println!("read: {:?}", t.tags),
        Err(e) => println!("READ ERROR: {e}"),
    }
    let Ok(mut file) = before else { return };
    file.tags.insert(TagField::Genre, "Edited".to_string());
    match TagEngine::write(&file) {
        Ok(()) => {}
        Err(e) => {
            println!("WRITE ERROR: {e}");
            return;
        }
    }
    let raw = std::fs::read(&path).unwrap();
    let after = TagEngine::read(&path).unwrap();
    println!("after unrelated edit: {:?}", after.tags);
    for (label, needle) in probes {
        println!(
            "  original bytes of {label} still in file: {}",
            find(&raw, needle)
        );
    }
    let changed: Vec<_> = file
        .tags
        .iter()
        .filter(|(k, v)| **k != TagField::Genre && after.tags.get(k) != Some(v))
        .collect();
    println!("  fields whose value changed: {changed:?}");
    std::fs::remove_file(&path).ok();
}

#[test]
#[ignore = "audit harness: prints findings, run with --ignored --nocapture"]
fn audit_encodings() {
    let audio = mp3_frames();
    // ID3v2.3: Latin-1 frame holding cp1251 bytes, UTF-16 with BOM in both orders.
    let mut v23 = tag(
        3,
        &[
            frame(3, "TIT2", 0, KINO_1251),
            frame(3, "TPE1", 1, &utf16le_bom("Цой")),
            frame(3, "TALB", 1, &utf16be_bom("Группа крови")),
        ],
    );
    v23.extend_from_slice(&audio);
    case(
        "v23-mixed",
        v23,
        &[
            ("TIT2 cp1251", KINO_1251),
            ("TPE1 utf16le", &utf16le_bom("Цой")),
        ],
    );

    // ID3v2.4: UTF-16BE without a BOM, UTF-8, and Latin-1 cp1251.
    let mut v24 = tag(
        4,
        &[
            frame(4, "TIT2", 2, &utf16be("Звезда")),
            frame(4, "TPE1", 3, "Кино".as_bytes()),
            frame(4, "TALB", 0, KINO_1251),
        ],
    );
    v24.extend_from_slice(&audio);
    case("v24-mixed", v24, &[("TALB cp1251", KINO_1251)]);

    // A lone ID3v1 tag in cp1251, as old rippers wrote it.
    let mut v1 = audio.clone();
    v1.extend_from_slice(&id3v1(KINO_1251, KINO_1251));
    case("id3v1-cp1251", v1, &[("ID3v1 cp1251", KINO_1251)]);

    // UTF-16 frame whose text has no terminator and odd length (a broken writer).
    let mut odd = tag(3, &[frame(3, "TIT2", 1, &[0xFF, 0xFE, 0x41, 0x00, 0x42])]);
    odd.extend_from_slice(&audio);
    case("v23-odd-utf16", odd, &[]);

    // UTF-16 with no BOM in a v2.3 frame (not allowed, but seen).
    let mut nobom = tag(3, &[frame(3, "TIT2", 1, &utf16be("Без BOM"))]);
    nobom.extend_from_slice(&audio);
    case("v23-utf16-no-bom", nobom, &[]);
}
