//! Deterministic mutation fuzz of the read/write paths. Reports panics, slow
//! calls and writes that leave a file unreadable.
//! Ignored by default. Run: cargo test --release -p tagrex-core --test audit_fuzz -- --nocapture

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use tagrex_core::model::{AudioFormat, TagEngine, TagField, TrackFile};

const MINIMAL_M4A: &[u8] = include_bytes!("fixtures/minimal.m4a");

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
fn mp3_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..20 {
        let mut frame = vec![0u8; 417];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        out.extend_from_slice(&frame);
    }
    out
}
fn wav_bytes() -> Vec<u8> {
    let mut v = b"RIFF".to_vec();
    v.extend_from_slice(&(36u32 + 8).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&[1, 0, 2, 0, 0x44, 0xac, 0, 0, 0x10, 0xb1, 2, 0, 4, 0, 16, 0]);
    v.extend_from_slice(b"data");
    v.extend_from_slice(&8u32.to_le_bytes());
    v.extend_from_slice(&[0u8; 8]);
    v
}
fn ogg_like() -> Vec<u8> {
    // Not a valid stream; exercises the probe/garbage path.
    let mut v = b"OggS".to_vec();
    v.extend_from_slice(&[0u8; 60]);
    v
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn mutate(base: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut b = base.to_vec();
    match rng.below(6) {
        0 => {
            for _ in 0..1 + rng.below(8) {
                let i = rng.below(b.len());
                b[i] = rng.next() as u8;
            }
        }
        1 => b.truncate(rng.below(b.len() + 1)),
        2 => {
            let i = rng.below(b.len());
            let n = rng.below(32);
            let fill = rng.next() as u8;
            b.splice(i..i, std::iter::repeat_n(fill, n));
        }
        3 => {
            // set a random 4-byte window to a hostile length
            if b.len() > 8 {
                let i = rng.below(b.len() - 4);
                let v = [0xFFu32, 0x7FFF_FFFF, u32::MAX, 0, 1][rng.below(5)];
                b[i..i + 4].copy_from_slice(&v.to_be_bytes());
            }
        }
        4 => {
            let i = rng.below(b.len());
            let j = rng.below(b.len());
            let (lo, hi) = (i.min(j), i.max(j));
            b.drain(lo..hi);
        }
        _ => {
            let i = rng.below(b.len());
            let n = rng.below(b.len() - i + 1).min(64);
            let chunk: Vec<u8> = b[i..i + n].to_vec();
            b.splice(i..i, chunk);
        }
    }
    b
}

fn run(label: &str, ext: &str, format: AudioFormat, seed_bytes: Vec<u8>, iterations: usize) {
    // Seed with a real tagged file so tag blocks exist to be corrupted.
    let seed_path = std::env::temp_dir().join(format!(
        "tagrex-fuzz-seed-{label}-{}.{ext}",
        std::process::id()
    ));
    std::fs::write(&seed_path, &seed_bytes).unwrap();
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Seed Artist".to_string());
    tags.insert(TagField::Title, "Seed Title".to_string());
    tags.insert(TagField::TrackNumber, "3".to_string());
    tags.insert(TagField::Custom("FOO".into()), "bar".to_string());
    let _ = TagEngine::write(&TrackFile {
        path: seed_path.clone(),
        format,
        tags,
    });
    let seed = std::fs::read(&seed_path).unwrap();
    std::fs::remove_file(&seed_path).ok();

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ label.len() as u64);
    let path =
        std::env::temp_dir().join(format!("tagrex-fuzz-{label}-{}.{ext}", std::process::id()));
    let (mut read_panics, mut write_panics, mut slow, mut poisoned, mut orig_changed_on_err) =
        (0, 0, 0, 0, 0);
    let mut first: Vec<String> = Vec::new();
    for it in 0..iterations {
        let data = mutate(&seed, &mut rng);
        std::fs::write(&path, &data).unwrap();

        let t = Instant::now();
        let r = std::panic::catch_unwind(|| TagEngine::read(&path));
        if t.elapsed() > Duration::from_secs(2) {
            slow += 1;
            first.push(format!("SLOW read {:?} iter {it}", t.elapsed()));
        }
        let readable = match r {
            Err(_) => {
                read_panics += 1;
                first.push(format!("PANIC read iter {it} len {}", data.len()));
                std::fs::write(
                    std::env::temp_dir().join(format!("tagrex-fuzz-panic-read-{label}-{it}.bin")),
                    &data,
                )
                .ok();
                false
            }
            Ok(Ok(_)) => true,
            Ok(Err(_)) => false,
        };

        // Write onto whatever it is, then check outcome class.
        let mut tags = BTreeMap::new();
        tags.insert(TagField::Artist, "Fuzzed".to_string());
        tags.insert(TagField::Album, "Alb".to_string());
        let t = Instant::now();
        let w = std::panic::catch_unwind(|| {
            TagEngine::write(&TrackFile {
                path: path.clone(),
                format,
                tags,
            })
        });
        if t.elapsed() > Duration::from_secs(2) {
            slow += 1;
            first.push(format!("SLOW write {:?} iter {it}", t.elapsed()));
        }
        match w {
            Err(_) => {
                write_panics += 1;
                first.push(format!("PANIC write iter {it} len {}", data.len()));
                std::fs::write(
                    std::env::temp_dir().join(format!("tagrex-fuzz-panic-write-{label}-{it}.bin")),
                    &data,
                )
                .ok();
            }
            Ok(Ok(())) => {
                // A successful write must leave a readable file when it was readable before.
                if readable && TagEngine::read(&path).is_err() {
                    poisoned += 1;
                    first.push(format!(
                        "POISONED iter {it}: readable before, unreadable after successful write"
                    ));
                    std::fs::write(
                        std::env::temp_dir().join(format!("tagrex-fuzz-poison-{label}-{it}.bin")),
                        &data,
                    )
                    .ok();
                }
            }
            Ok(Err(_)) => {
                if std::fs::read(&path).unwrap() != data {
                    orig_changed_on_err += 1;
                    first.push(format!("ERR BUT FILE CHANGED iter {it}"));
                    std::fs::write(
                        std::env::temp_dir().join(format!("tagrex-fuzz-errchg-{label}-{it}.bin")),
                        &data,
                    )
                    .ok();
                }
            }
        }
    }
    std::fs::remove_file(&path).ok();
    println!(
        "\n===== fuzz {label}: {iterations} iters | read panics {read_panics} | write panics {write_panics} | slow {slow} | poisoned {poisoned} | err-but-changed {orig_changed_on_err} ====="
    );
    for f in first.iter().take(15) {
        println!("  {f}");
    }
}

#[test]
#[ignore = "audit harness: prints findings, run with --ignored --nocapture"]
fn fuzz_all() {
    let n = 4000;
    run("flac", "flac", AudioFormat::Flac, flac_bytes(), n);
    run("mp3", "mp3", AudioFormat::Mp3, mp3_bytes(), n);
    run("m4a", "m4a", AudioFormat::M4a, MINIMAL_M4A.to_vec(), n);
    run("wav", "wav", AudioFormat::Wav, wav_bytes(), n);
    run("ogg", "ogg", AudioFormat::OggVorbis, ogg_like(), n);
}

/// Seeds from real files (made by an outside encoder), one run per file in
/// `TAGREX_AUDIT_IN`; covers the containers the synthetic seeds above cannot
/// build by hand (Ogg Vorbis, Opus, Speex, AIFF, WavPack, MP4).
#[test]
#[ignore = "audit harness: prints findings, run with --ignored --nocapture"]
fn fuzz_real_files() {
    let Ok(dir) = std::env::var("TAGREX_AUDIT_IN") else {
        println!("TAGREX_AUDIT_IN not set");
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    for path in files {
        let Ok(read) = TagEngine::read(&path) else {
            continue;
        };
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let ext = name.rsplit('.').next().unwrap().to_string();
        run(
            &name,
            &ext,
            read.format,
            std::fs::read(&path).unwrap(),
            1500,
        );
    }
}
