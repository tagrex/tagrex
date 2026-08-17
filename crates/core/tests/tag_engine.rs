//! End-to-end read/write smoke test against a real (synthetic) FLAC file.
//!
//! There's no test-fixture audio in the repo yet, so this builds the
//! smallest file FLAC parsers accept by hand: the `fLaC` magic plus a single
//! STREAMINFO metadata block, no audio frames. Enough for lofty to identify
//! the format and read/write a Vorbis Comments block.

use std::collections::BTreeMap;
use std::path::PathBuf;

use tagrex_core::model::{AudioFormat, CoverArt, TagEngine, TagField, TrackFile};

/// `fLaC` + a non-last STREAMINFO block (34 bytes, all zeroed out except a
/// plausible sample rate/channels/bit depth) + a trailing PADDING block.
///
/// STREAMINFO-only files (no padding) make lofty's FLAC writer index past
/// the metadata block when it tries to splice one in, since it assumes
/// there's always at least a padding block to reuse or resize — true of
/// every file a real encoder produces, but not of a hand-built minimal one.
/// Shaping the fixture the way real encoders do (metadata always followed by
/// padding) avoids that and exercises the same code path real files take.
const MINIMAL_FLAC: [u8; 62] = [
    0x66, 0x4c, 0x61, 0x43, 0x00, 0x00, 0x00,
    0x22, // "fLaC" + STREAMINFO header (not last), length 34
    0x10, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00,
    0x00, // min/max blocksize = 4096, min/max frame size = 0
    0x00, 0x00, 0x0a, 0xc4, 0x42, 0xf0, 0x00, 0x00, // 44100 Hz, 2ch, 16 bps, 0 samples
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // MD5 signature (zeroed)
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x81, 0x00, 0x00, 0x10, 0x00,
    0x00, // PADDING header (last), length 16
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // padding content
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn temp_flac_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "tagrex-tag-engine-test-{name}-{}.flac",
        std::process::id()
    ));
    path
}

#[test]
fn write_then_read_round_trips_known_and_custom_fields() {
    let path = temp_flac_path("round-trip");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");

    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Test Artist".to_string());
    tags.insert(TagField::Title, "Test Title".to_string());
    // Year must survive a write: it's stored via RecordingDate, since a plain
    // "year" isn't a real ID3v2.4 frame (regression guard).
    tags.insert(TagField::Year, "1996".to_string());
    // Deliberately not a key lofty recognizes for Vorbis Comments (unlike,
    // say, "MOOD" or "COMPOSER") — a recognized key would round-trip back as
    // its matching `ItemKey` variant, not as this literal `Custom` string.
    tags.insert(
        TagField::Custom("TAGREX_CUSTOM_TEST".to_string()),
        "Energetic".to_string(),
    );

    let file = TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags,
    };

    TagEngine::write(&file).expect("write tags");

    let read_back = TagEngine::read(&path).expect("read tags");
    std::fs::remove_file(&path).ok();

    assert_eq!(read_back.format, AudioFormat::Flac);
    assert_eq!(
        read_back.tags.get(&TagField::Artist).map(String::as_str),
        Some("Test Artist")
    );
    assert_eq!(
        read_back.tags.get(&TagField::Title).map(String::as_str),
        Some("Test Title")
    );
    assert_eq!(
        read_back.tags.get(&TagField::Year).map(String::as_str),
        Some("1996")
    );
    assert_eq!(
        read_back
            .tags
            .get(&TagField::Custom("TAGREX_CUSTOM_TEST".to_string()))
            .map(String::as_str),
        Some("Energetic")
    );
}

/// #165: the tempo used to be written under ID3v2's integer-BPM item on every
/// format. Vorbis Comments have no mapping for it, so the item was dropped when
/// the tag was saved and the value simply was not in the file afterwards — no
/// error anywhere. The DJ fields are the whole point of the online sources that
/// state them, so both spellings get a real round trip here, on the two tag
/// types that can hold them.
#[test]
fn the_dj_fields_survive_a_round_trip_on_a_vorbis_file() {
    let path = temp_flac_path("dj-fields");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");

    let mut tags = BTreeMap::new();
    tags.insert(TagField::Bpm, "128".to_string());
    tags.insert(TagField::InitialKey, "Am".to_string());
    // The label went the same way on MP4 and is fixed with it; on Vorbis it
    // always worked, and must keep working.
    tags.insert(TagField::Publisher, "Test Records".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags,
    })
    .expect("write tags");

    let read_back = TagEngine::read(&path).expect("read tags");
    std::fs::remove_file(&path).ok();
    assert_eq!(
        read_back.tags.get(&TagField::Bpm).map(String::as_str),
        Some("128")
    );
    assert_eq!(
        read_back
            .tags
            .get(&TagField::InitialKey)
            .map(String::as_str),
        Some("Am")
    );
    assert_eq!(
        read_back.tags.get(&TagField::Publisher).map(String::as_str),
        Some("Test Records")
    );
}

/// The same fields on an ID3v2 container, which is where they always worked —
/// the fix must not move them off `TBPM`, the frame DJ software actually reads.
#[test]
fn the_dj_fields_survive_a_round_trip_on_an_id3v2_file() {
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-dj-fields-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");

    let mut tags = BTreeMap::new();
    tags.insert(TagField::Bpm, "128".to_string());
    tags.insert(TagField::InitialKey, "Am".to_string());
    tags.insert(TagField::Publisher, "Test Records".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Mp3,
        tags,
    })
    .expect("write tags");

    let read_back = TagEngine::read(&path).expect("read tags");
    // The tempo must be in TBPM specifically, not a TXXX text frame.
    let has_tbpm = {
        use lofty::config::ParseOptions;
        use lofty::file::AudioFile;
        use lofty::mpeg::MpegFile;
        let mut file = std::fs::File::open(&path).unwrap();
        MpegFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .id3v2()
            .map(|tag| tag.into_iter().any(|frame| frame.id().as_str() == "TBPM"))
            .unwrap_or(false)
    };
    std::fs::remove_file(&path).ok();

    assert_eq!(
        read_back.tags.get(&TagField::Bpm).map(String::as_str),
        Some("128")
    );
    assert_eq!(
        read_back
            .tags
            .get(&TagField::InitialKey)
            .map(String::as_str),
        Some("Am")
    );
    assert_eq!(
        read_back.tags.get(&TagField::Publisher).map(String::as_str),
        Some("Test Records")
    );
    assert!(has_tbpm, "the tempo left TBPM, which is what DJ tools read");
}

/// #171: another tagger's user-text spellings of fields that have a standard
/// frame. A file can carry both — its `TXXX:Label` beside the `TPUB` an import
/// wrote — and used to show them as two contradictory fields.
#[test]
fn a_legacy_user_text_field_folds_into_the_standard_one() {
    use lofty::config::WriteOptions;
    use lofty::id3::v2::{ExtendedTextFrame, Frame, FrameId, Id3v2Tag, TextInformationFrame};
    use lofty::prelude::TagExt;
    use lofty::TextEncoding;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-legacy-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");

    // Both spellings, as a real file has them: the standard frame with the
    // current value, the legacy user-text one with a stale value.
    let mut seeded = Id3v2Tag::new();
    seeded.insert(Frame::Text(TextInformationFrame::new(
        FrameId::Valid("TPUB".into()),
        TextEncoding::UTF8,
        "New Label".to_string(),
    )));
    seeded.insert(Frame::UserText(ExtendedTextFrame::new(
        TextEncoding::UTF8,
        "Label".to_string(),
        "Stale Label".to_string(),
    )));
    seeded.insert(Frame::Text(TextInformationFrame::new(
        FrameId::Valid("TMED".into()),
        TextEncoding::UTF8,
        "Digital".to_string(),
    )));
    seeded.insert(Frame::UserText(ExtendedTextFrame::new(
        TextEncoding::UTF8,
        "OriginalMediaType".to_string(),
        "Cd".to_string(),
    )));
    // Only the old spelling: its value carries into the standard field.
    seeded.insert(Frame::UserText(ExtendedTextFrame::new(
        TextEncoding::UTF8,
        "COUNTRY".to_string(),
        "Belgium".to_string(),
    )));
    // And something we don't understand, which must keep round-tripping.
    seeded.insert(Frame::UserText(ExtendedTextFrame::new(
        TextEncoding::UTF8,
        "TAGREX_CUSTOM_TEST".to_string(),
        "Energetic".to_string(),
    )));
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();

    let read_back = TagEngine::read(&path).expect("read tags");
    // The standard frame wins over the stale twin.
    assert_eq!(
        read_back.tags.get(&TagField::Publisher).map(String::as_str),
        Some("New Label")
    );
    assert_eq!(
        read_back.tags.get(&TagField::MediaType).map(String::as_str),
        Some("Digital")
    );
    // The old spelling is gone as a field of its own.
    for stale in ["Label", "OriginalMediaType", "COUNTRY"] {
        assert_eq!(
            read_back.tags.get(&TagField::Custom(stale.to_string())),
            None,
            "{stale} still shows as a field of its own"
        );
    }
    // A lone legacy value carries into the field it means.
    assert_eq!(
        read_back
            .tags
            .get(&TagField::Custom("RELEASECOUNTRY".to_string()))
            .map(String::as_str),
        Some("Belgium")
    );
    // An unrecognized user-text field is untouched.
    assert_eq!(
        read_back
            .tags
            .get(&TagField::Custom("TAGREX_CUSTOM_TEST".to_string()))
            .map(String::as_str),
        Some("Energetic")
    );

    // Writing consolidates: the stale frames are not carried over, so the file
    // stops contradicting itself.
    TagEngine::write(&read_back).expect("write tags");
    let after = TagEngine::read(&path).expect("read tags");
    std::fs::remove_file(&path).ok();
    assert_eq!(
        after.tags.get(&TagField::Publisher).map(String::as_str),
        Some("New Label")
    );
    assert_eq!(
        after
            .tags
            .get(&TagField::Custom("RELEASECOUNTRY".to_string()))
            .map(String::as_str),
        Some("Belgium")
    );
    for stale in ["Label", "OriginalMediaType", "COUNTRY"] {
        assert_eq!(after.tags.get(&TagField::Custom(stale.to_string())), None);
    }
}

/// #172: the file listing needs the playing time next to the tags, and parsing
/// every file twice to get it would double the cost of opening a library. One
/// probe answers both, and `read` is that call minus the properties — so the two
/// must agree about the tags.
#[test]
fn one_probe_returns_both_the_tags_and_the_properties() {
    let path = temp_flac_path("with-props");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Test Artist".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags,
    })
    .expect("write tags");

    let (track, props) = TagEngine::read_with_props(&path).expect("read");
    let plain = TagEngine::read(&path).expect("read");
    std::fs::remove_file(&path).ok();

    assert_eq!(track.tags, plain.tags);
    assert_eq!(track.format, AudioFormat::Flac);
    assert_eq!(
        track.tags.get(&TagField::Artist).map(String::as_str),
        Some("Test Artist")
    );
    // The fixture's STREAMINFO states the stream, so the properties are real
    // values rather than a default-constructed blank.
    assert_eq!(props.sample_rate_hz, Some(44100));
    assert_eq!(props.channels, Some(2));
    // It carries no audio frames, so it is genuinely zero seconds long.
    assert_eq!(props.duration_secs, 0);
}

/// An ID3v2.4 tag carrying a three-digit year, built by hand because the backend
/// refuses to write one — which is the whole point of #183: files like this
/// exist, other software reads them, and ours used to refuse the entire file
/// over that one frame. `TDRC` holds "199"; `TIT2` and `TPE1` are what has to
/// survive the frame that cannot be parsed.
fn mp3_with_a_broken_year() -> Vec<u8> {
    fn frame(id: &str, text: &str) -> Vec<u8> {
        let mut body = vec![0x03]; // UTF-8 encoding byte
        body.extend_from_slice(text.as_bytes());
        let mut out = id.as_bytes().to_vec();
        // ID3v2.4 sizes are syncsafe: 7 bits per byte.
        let size = body.len() as u32;
        out.extend_from_slice(&[
            ((size >> 21) & 0x7f) as u8,
            ((size >> 14) & 0x7f) as u8,
            ((size >> 7) & 0x7f) as u8,
            (size & 0x7f) as u8,
        ]);
        out.extend_from_slice(&[0x00, 0x00]); // frame flags
        out.extend_from_slice(&body);
        out
    }

    let mut frames = Vec::new();
    frames.extend(frame("TIT2", "Drinking Chardonnay"));
    frames.extend(frame("TPE1", "DJ Lapell"));
    frames.extend(frame("TDRC", "199"));

    let mut tag = b"ID3".to_vec();
    tag.extend_from_slice(&[0x04, 0x00, 0x00]); // v2.4, no flags
    let size = frames.len() as u32;
    tag.extend_from_slice(&[
        ((size >> 21) & 0x7f) as u8,
        ((size >> 14) & 0x7f) as u8,
        ((size >> 7) & 0x7f) as u8,
        (size & 0x7f) as u8,
    ]);
    tag.extend_from_slice(&frames);
    tag.extend_from_slice(&minimal_mp3());
    tag
}

#[test]
fn a_file_with_an_unreadable_year_still_opens() {
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-bad-year-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, mp3_with_a_broken_year()).expect("write fixture");

    let track = TagEngine::read(&path).expect("a bad year must not hide the file");
    assert_eq!(
        track.tags.get(&TagField::Title).map(String::as_str),
        Some("Drinking Chardonnay")
    );
    assert_eq!(
        track.tags.get(&TagField::Artist).map(String::as_str),
        Some("DJ Lapell")
    );
    // The frame that could not be parsed is absent rather than guessed at.
    assert_eq!(track.tags.get(&TagField::Year), None);

    // And the file is repairable: an ordinary write rebuilds the text frames
    // from the model, so the broken one is gone afterwards.
    let mut fixed = track.clone();
    fixed.tags.insert(TagField::Year, "1996".to_string());
    TagEngine::write(&fixed).expect("write tags");
    let after = TagEngine::read(&path).expect("read back");
    std::fs::remove_file(&path).ok();
    assert_eq!(
        after.tags.get(&TagField::Year).map(String::as_str),
        Some("1996")
    );
    assert_eq!(
        after.tags.get(&TagField::Title).map(String::as_str),
        Some("Drinking Chardonnay")
    );
}

#[test]
fn cover_embed_read_remove_and_survives_a_tag_write() {
    let path = temp_flac_path("cover");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");

    // Arbitrary bytes stand in for image data (lofty stores them verbatim).
    let cover = CoverArt {
        mime: "image/png".to_string(),
        data: vec![0x89, 0x50, 0x4e, 0x47, 1, 2, 3, 4, 5],
        ..CoverArt::default()
    };

    // No cover initially.
    assert_eq!(TagEngine::read_cover(&path).unwrap(), None);

    // Embed, then read it back.
    TagEngine::write_covers(&path, std::slice::from_ref(&cover)).unwrap();
    let read = TagEngine::read_cover(&path)
        .unwrap()
        .expect("cover present");
    assert_eq!(read.mime, "image/png");
    assert_eq!(read.data, cover.data);

    // A subsequent tag write must NOT strip the cover.
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Someone".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags,
    })
    .unwrap();
    assert_eq!(
        TagEngine::read_cover(&path).unwrap().map(|c| c.data),
        Some(cover.data.clone())
    );

    // Remove it.
    TagEngine::write_covers(&path, &[]).unwrap();
    assert_eq!(TagEngine::read_cover(&path).unwrap(), None);

    std::fs::remove_file(&path).ok();
}

/// A tag write (and a cover embed) must preserve everything the text-only
/// `TagMap` cannot express: DJ cue points and loops, ratings, ReplayGain — all
/// of which live in ID3v2 frames like `PRIV`/`GEOB`.
///
/// This is MP3-specific on purpose: lofty's *generic* `Tag` does not surface
/// those frames at all, so an MP3 round-tripped through it loses them silently.
/// Vorbis Comments cannot hold binary data in the first place, so FLAC can't
/// exercise this.
#[test]
fn mp3_write_preserves_non_text_frames() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Id3v2Tag, PrivateFrame};
    use lofty::mpeg::MpegFile;
    use lofty::prelude::{Accessor, TagExt};

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-preserve-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).unwrap();

    // Seed a private frame, standing in for what DJ software writes.
    let mut seeded = Id3v2Tag::new();
    seeded.set_artist("Original".to_string());
    seeded.insert(PrivateFrame::new("SeratoMarkers".to_string(), vec![9, 8, 7, 6]).into());
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();

    let private_data = |path: &PathBuf| -> Option<Vec<u8>> {
        let mut file = std::fs::File::open(path).unwrap();
        let mpeg = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
        mpeg.id3v2().and_then(|tag| {
            tag.into_iter().find_map(|frame| match frame {
                lofty::id3::v2::Frame::Private(p) => Some(p.private_data.clone()),
                _ => None,
            })
        })
    };
    assert_eq!(
        private_data(&path),
        Some(vec![9, 8, 7, 6]),
        "seeding failed"
    );

    // A normal tag edit through the engine.
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Edited".to_string());
    tags.insert(TagField::Title, "New Title".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Mp3,
        tags,
    })
    .unwrap();

    // The text edit landed...
    let read = TagEngine::read(&path).unwrap();
    assert_eq!(
        read.tags.get(&TagField::Artist).map(String::as_str),
        Some("Edited")
    );
    // ...and the private frame survived it.
    assert_eq!(
        private_data(&path),
        Some(vec![9, 8, 7, 6]),
        "tag write destroyed the private frame"
    );

    // Embedding a cover must not destroy it either.
    TagEngine::write_covers(
        &path,
        &[CoverArt {
            mime: "image/png".to_string(),
            data: vec![1, 2, 3, 4],
            ..CoverArt::default()
        }],
    )
    .unwrap();
    assert_eq!(
        private_data(&path),
        Some(vec![9, 8, 7, 6]),
        "cover embed destroyed the private frame"
    );
    assert!(TagEngine::read_cover(&path).unwrap().is_some());

    std::fs::remove_file(&path).ok();
}

/// A few MPEG-1 Layer III frames (128 kbps, 44.1 kHz) — enough for lofty to
/// identify the file and attach tags to it.
fn minimal_mp3() -> Vec<u8> {
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
    frame.resize(417, 0);
    let mut data = Vec::new();
    for _ in 0..5 {
        data.extend_from_slice(&frame);
    }
    data
}

/// The same frame-preservation guarantee must hold for the other ID3v2
/// containers, not just MP3 — AIFF and WAV are exactly where DJ software writes
/// cue points, so adding those formats without this would risk the very data
/// loss #52 fixed.
#[test]
fn wav_write_preserves_non_text_frames() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, Id3v2Tag, PrivateFrame};
    use lofty::iff::wav::WavFile;
    use lofty::prelude::{Accessor, TagExt};

    let path =
        std::env::temp_dir().join(format!("tagrex-tag-engine-wav-{}.wav", std::process::id()));
    std::fs::write(&path, minimal_wav()).unwrap();

    let mut seeded = Id3v2Tag::new();
    seeded.set_artist("Original".to_string());
    seeded.insert(PrivateFrame::new("SeratoMarkers".to_string(), vec![5, 6, 7]).into());
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();

    let private_data = |path: &PathBuf| -> Option<Vec<u8>> {
        let mut file = std::fs::File::open(path).unwrap();
        let wav = WavFile::read_from(&mut file, ParseOptions::new()).unwrap();
        wav.id3v2().and_then(|tag| {
            tag.into_iter().find_map(|frame| match frame {
                Frame::Private(p) => Some(p.private_data.clone()),
                _ => None,
            })
        })
    };
    assert_eq!(private_data(&path), Some(vec![5, 6, 7]), "seeding failed");

    // The engine must recognise WAV and route it through the concrete tag.
    let read = TagEngine::read(&path).unwrap();
    assert_eq!(read.format, AudioFormat::Wav);

    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Edited".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Wav,
        tags,
    })
    .unwrap();

    assert_eq!(
        TagEngine::read(&path)
            .unwrap()
            .tags
            .get(&TagField::Artist)
            .map(String::as_str),
        Some("Edited")
    );
    assert_eq!(
        private_data(&path),
        Some(vec![5, 6, 7]),
        "tag write destroyed the private frame in a WAV"
    );

    std::fs::remove_file(&path).ok();
}

/// A read priority that names blocks the file doesn't carry must fall back to
/// the present one, not drop the tags (#84). This FLAC has only a Vorbis block;
/// prioritizing ID3v2/APE (both absent) should still read the Vorbis values.
#[test]
fn read_priority_falls_back_to_present_block() {
    let path = temp_flac_path("read-priority");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");

    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Vorbis Artist".to_string());
    tags.insert(TagField::Title, "Vorbis Title".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags,
    })
    .expect("write vorbis tags");

    tagrex_core::model::set_read_priority(&["id3v2".to_string(), "ape".to_string()]);
    let read = TagEngine::read(&path).expect("read back");
    // Reset the process-global before asserting so a failure can't leak it into
    // other tests sharing this binary.
    tagrex_core::model::set_read_priority(&[]);

    assert_eq!(
        read.tags.get(&TagField::Artist).map(String::as_str),
        Some("Vorbis Artist"),
        "an absent prioritized block should fall back to the present one"
    );

    std::fs::remove_file(&path).ok();
}

/// Several artists and genres on one track survive a round trip, as a repeated
/// Vorbis comment (#46).
///
/// This is the data loss the issue is about: the read loop used to overwrite,
/// so a FLAC with `ARTIST=Autechre` and `ARTIST=Gescom` came back as `Gescom`
/// alone, and the next write put that one value back over both. A field that is
/// not multi-valued must NOT be split, or a title containing the separator
/// would silently become two titles.
#[test]
fn multi_value_fields_round_trip_as_repeated_entries() {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::flac::FlacFile;

    let path = temp_flac_path("multi-value");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");

    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Autechre; Gescom".to_string());
    tags.insert(TagField::Genre, "Electronic; IDM; Ambient".to_string());
    // Not a multi-value field: the separator inside it is part of the title.
    tags.insert(TagField::Title, "Hello; Goodbye".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags,
    })
    .expect("write tags");

    // On disk it is genuinely several comments, not one joined string — that is
    // what other players and taggers have to see for this to mean anything.
    let entries = |key: &str| -> Vec<String> {
        let mut file = std::fs::File::open(&path).unwrap();
        let flac = FlacFile::read_from(&mut file, ParseOptions::new()).unwrap();
        flac.vorbis_comments()
            .unwrap()
            .get_all(key)
            .map(str::to_string)
            .collect()
    };
    assert_eq!(entries("ARTIST"), vec!["Autechre", "Gescom"]);
    assert_eq!(entries("GENRE"), vec!["Electronic", "IDM", "Ambient"]);
    assert_eq!(entries("TITLE"), vec!["Hello; Goodbye"]);

    // And it reads back as the one string the rest of the app works with.
    let read = TagEngine::read(&path).expect("read back");
    assert_eq!(
        read.tags.get(&TagField::Artist).map(String::as_str),
        Some("Autechre; Gescom")
    );
    assert_eq!(
        read.tags.get(&TagField::Genre).map(String::as_str),
        Some("Electronic; IDM; Ambient")
    );
    assert_eq!(
        read.tags.get(&TagField::Title).map(String::as_str),
        Some("Hello; Goodbye")
    );

    // A second write must not accumulate duplicates.
    TagEngine::write(&read).expect("rewrite");
    assert_eq!(entries("ARTIST"), vec!["Autechre", "Gescom"]);

    std::fs::remove_file(&path).ok();
}

/// The ID3v2 side of the same guarantee (#46): several values go into ONE
/// multi-value frame, which is what ID3v2.4 specifies — not duplicate TPE1
/// frames, which are out of spec and which most players ignore.
#[test]
fn mp3_multi_value_fields_become_one_frame_with_several_values() {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::mpeg::MpegFile;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-multi-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).unwrap();

    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Autechre; Gescom".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Mp3,
        tags,
    })
    .unwrap();

    let mut file = std::fs::File::open(&path).unwrap();
    let mpeg = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
    let artist_frames: Vec<String> = mpeg
        .id3v2()
        .unwrap()
        .into_iter()
        .filter(|frame| frame.id().as_str() == "TPE1")
        .map(|frame| match frame {
            lofty::id3::v2::Frame::Text(text) => text.value.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(artist_frames.len(), 1, "one TPE1 frame, not several");
    // ID3v2.4 separates the values inside the frame with a null byte.
    assert_eq!(artist_frames[0], "Autechre\0Gescom");

    assert_eq!(
        TagEngine::read(&path)
            .unwrap()
            .tags
            .get(&TagField::Artist)
            .map(String::as_str),
        Some("Autechre; Gescom")
    );

    std::fs::remove_file(&path).ok();
}

/// Several images with their types round-trip, and the whole set is what a
/// write replaces (#56). The front-cover reader keeps picking the front one out
/// of the set, which is what "the cover" means everywhere else in the app.
#[test]
fn several_images_round_trip_with_their_types() {
    use tagrex_core::model::CoverKind;

    let path = temp_flac_path("cover-set");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");

    let image = |kind: CoverKind, byte: u8, description: &str| CoverArt {
        mime: "image/png".to_string(),
        data: vec![0x89, 0x50, 0x4e, 0x47, byte],
        kind,
        description: description.to_string(),
    };
    // Deliberately not front-first, so the order is the set's and not a
    // by-product of how the writer sorts.
    let set = vec![
        image(CoverKind::Back, 2, ""),
        image(CoverKind::Front, 1, "the sleeve"),
        image(CoverKind::Media, 3, ""),
    ];
    TagEngine::write_covers(&path, &set).unwrap();

    let read = TagEngine::read_covers(&path).unwrap();
    assert_eq!(read, set, "types, descriptions and order must all survive");
    // The single-cover reader still answers with the front one, not the first.
    let front = TagEngine::read_cover(&path)
        .unwrap()
        .expect("a front cover");
    assert_eq!(front.kind, CoverKind::Front);
    assert_eq!(front.description, "the sleeve");

    // A write replaces the whole set rather than adding to it.
    let smaller = vec![image(CoverKind::Front, 9, "")];
    TagEngine::write_covers(&path, &smaller).unwrap();
    assert_eq!(TagEngine::read_covers(&path).unwrap(), smaller);

    // And an empty set removes every image.
    TagEngine::write_covers(&path, &[]).unwrap();
    assert!(TagEngine::read_covers(&path).unwrap().is_empty());
    assert_eq!(TagEngine::read_cover(&path).unwrap(), None);

    std::fs::remove_file(&path).ok();
}

/// The same on the ID3v2 side, where the write goes through the concrete tag —
/// and where clearing the old pictures must not take the private frames with it.
#[test]
fn mp3_keeps_several_images_and_its_private_frames() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, Id3v2Tag, PrivateFrame};
    use lofty::mpeg::MpegFile;
    use lofty::prelude::TagExt;
    use tagrex_core::model::CoverKind;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-cover-set-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).unwrap();

    let mut seeded = Id3v2Tag::new();
    seeded.insert(PrivateFrame::new("SeratoMarkers".to_string(), vec![4, 3, 2]).into());
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();

    let set = vec![
        CoverArt {
            mime: "image/png".to_string(),
            data: vec![0x89, 0x50, 1],
            kind: CoverKind::Front,
            description: String::new(),
        },
        CoverArt {
            mime: "image/png".to_string(),
            data: vec![0x89, 0x50, 2],
            kind: CoverKind::Back,
            description: String::new(),
        },
    ];
    TagEngine::write_covers(&path, &set).unwrap();
    assert_eq!(TagEngine::read_covers(&path).unwrap(), set);

    // Writing the set a second time must not accumulate pictures.
    TagEngine::write_covers(&path, &set).unwrap();
    assert_eq!(TagEngine::read_covers(&path).unwrap(), set);

    let mut file = std::fs::File::open(&path).unwrap();
    let mpeg = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
    let private = mpeg.id3v2().and_then(|tag| {
        tag.into_iter().find_map(|frame| match frame {
            Frame::Private(p) => Some(p.private_data.clone()),
            _ => None,
        })
    });
    assert_eq!(
        private,
        Some(vec![4, 3, 2]),
        "clearing the pictures destroyed the private frame"
    );

    std::fs::remove_file(&path).ok();
}

/// A minimal PCM WAV (8 kHz mono 8-bit silence) lofty accepts.
fn minimal_wav() -> Vec<u8> {
    let samples: u32 = 800;
    let mut data = Vec::new();
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36 + samples).to_le_bytes());
    data.extend_from_slice(b"WAVE");
    data.extend_from_slice(b"fmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    data.extend_from_slice(&1u16.to_le_bytes()); // mono
    data.extend_from_slice(&8000u32.to_le_bytes());
    data.extend_from_slice(&8000u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&8u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&samples.to_le_bytes());
    data.resize(data.len() + samples as usize, 128);
    data
}

/// #194: a file carrying both an ID3v2 and a legacy ID3v1 tag had only the
/// ID3v2 one updated, so the two disagreed — invisibly, until the ID3v2 tag was
/// cleared and the stale ID3v1 became the file's only answer, reading back as if
/// nothing had been cleared (with every value cut to ID3v1's 30 bytes).
#[test]
fn a_write_keeps_an_existing_id3v1_tag_in_step() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v1::Id3v1Tag;
    use lofty::mpeg::MpegFile;
    use lofty::prelude::TagExt;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-id3v1-sync-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");

    // The state a real file arrives in: an ID3v1 tag written by something older.
    let legacy = Id3v1Tag {
        artist: Some("Stale Artist".to_string()),
        title: Some("Stale Title".to_string()),
        ..Default::default()
    };
    legacy
        .save_to_path(&path, WriteOptions::default())
        .expect("seed id3v1");

    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "New Artist".to_string());
    tags.insert(TagField::Title, "New Title".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Mp3,
        tags,
    })
    .expect("write tags");

    let id3v1 = {
        let mut file = std::fs::File::open(&path).unwrap();
        MpegFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .id3v1()
            .cloned()
    };
    std::fs::remove_file(&path).ok();

    let id3v1 = id3v1.expect("the file still has its ID3v1 tag");
    assert_eq!(id3v1.artist.as_deref(), Some("New Artist"));
    assert_eq!(id3v1.title.as_deref(), Some("New Title"));
}

/// The other half of #194: clearing the tags has to take the ID3v1 block with
/// it, or the file still answers with the old values.
#[test]
fn clearing_the_tags_removes_the_id3v1_tag() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v1::Id3v1Tag;
    use lofty::mpeg::MpegFile;
    use lofty::prelude::TagExt;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-id3v1-clear-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");

    let legacy = Id3v1Tag {
        artist: Some("Stale Artist".to_string()),
        title: Some("Stale Title".to_string()),
        album: Some("Stale Album".to_string()),
        ..Default::default()
    };
    legacy
        .save_to_path(&path, WriteOptions::default())
        .expect("seed id3v1");

    // What a cleared file looks like to the writer: nothing in the model at all.
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Mp3,
        tags: BTreeMap::new(),
    })
    .expect("write tags");

    let id3v1 = {
        let mut file = std::fs::File::open(&path).unwrap();
        MpegFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .id3v1()
            .cloned()
    };
    let read_back = TagEngine::read(&path).expect("read tags");
    std::fs::remove_file(&path).ok();

    assert!(id3v1.is_none(), "the stale ID3v1 tag survived the clear");
    assert_eq!(read_back.tags.get(&TagField::Artist), None);
    assert_eq!(read_back.tags.get(&TagField::Album), None);
}

/// And a file that never had one does not acquire a legacy tag on write —
/// nobody asked for it (#194).
#[test]
fn a_write_does_not_create_an_id3v1_tag() {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::mpeg::MpegFile;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-id3v1-none-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");

    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Only In ID3v2".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Mp3,
        tags,
    })
    .expect("write tags");

    let id3v1 = {
        let mut file = std::fs::File::open(&path).unwrap();
        MpegFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .id3v1()
            .cloned()
    };
    let read_back = TagEngine::read(&path).expect("read tags");
    std::fs::remove_file(&path).ok();

    assert!(id3v1.is_none(), "a legacy tag appeared out of nowhere");
    assert_eq!(
        read_back.tags.get(&TagField::Artist).map(String::as_str),
        Some("Only In ID3v2")
    );
}
