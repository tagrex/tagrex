//! Builds the seed corpus from small untagged audio files: each one is tagged
//! (text fields, a custom field, a cover) through the app's own writer, then
//! saved with the selector byte that `input_file` maps back to its extension.
//!
//!     cargo +nightly run --example seeds -- <untagged files...> <out dir>
//!
//! A seed that already carries tag blocks lets the fuzzer start inside them
//! instead of first having to invent their headers.

use std::collections::BTreeMap;
use std::path::Path;

use tagrex_core::model::{CoverArt, TagEngine, TagField};

/// A 1x1 PNG, enough for every format's picture frame.
const PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d, 0xb0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let out = args
        .pop()
        .expect("usage: seeds <untagged files...> <out dir>");
    std::fs::create_dir_all(&out).expect("create the output folder");
    let scratch = std::env::temp_dir().join(format!("tagrex-seeds-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create a scratch folder");

    for source in &args {
        let source = Path::new(source);
        let ext = source
            .extension()
            .and_then(|ext| ext.to_str())
            .expect("an input file has an extension");
        let Some(selector) = tagrex_fuzz::EXTENSIONS
            .iter()
            .position(|known| *known == ext)
        else {
            eprintln!("skipped {}: no selector for .{ext}", source.display());
            continue;
        };
        let work = scratch.join(source.file_name().expect("a file name"));
        std::fs::copy(source, &work).expect("copy the input");
        let mut file = match TagEngine::read(&work) {
            Ok(file) => file,
            Err(err) => {
                eprintln!("skipped {}: {err}", source.display());
                continue;
            }
        };
        let fields: BTreeMap<TagField, String> = [
            (TagField::Artist, "Seed Artist"),
            (TagField::Title, "Seed Title"),
            (TagField::Album, "Seed Album"),
            (TagField::TrackNumber, "3"),
            (TagField::Year, "2001"),
            (TagField::Comment, "seed comment"),
            (TagField::Bpm, "124"),
            (TagField::Custom("SEED".into()), "custom value"),
        ]
        .into_iter()
        .map(|(field, value)| (field, value.to_string()))
        .collect();
        file.tags.extend(fields);
        if let Err(err) = TagEngine::write(&file) {
            eprintln!("{}: tags not written: {err}", source.display());
        }
        let cover = CoverArt {
            mime: "image/png".to_string(),
            data: PIXEL_PNG.into(),
            ..CoverArt::default()
        };
        if let Err(err) = TagEngine::write_covers(&work, &[cover]) {
            eprintln!("{}: cover not written: {err}", source.display());
        }
        let mut seed = vec![selector as u8];
        seed.extend(std::fs::read(&work).expect("read the tagged file"));
        let name = format!("seed-{ext}");
        std::fs::write(Path::new(&out).join(name), seed).expect("write the seed");
    }
    std::fs::remove_dir_all(&scratch).ok();
}
