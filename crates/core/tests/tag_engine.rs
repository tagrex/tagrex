//! End-to-end read/write smoke test against a real (synthetic) FLAC file.
//!
//! There's no test-fixture audio in the repo yet, so this builds the
//! smallest file FLAC parsers accept by hand: the `fLaC` magic, a STREAMINFO
//! and a PADDING block, then one frame of silence. Enough for lofty to
//! identify the format and read/write a Vorbis Comments block.

use std::collections::BTreeMap;
use std::path::PathBuf;

use tagrex_core::model::{
    AudioFormat, CoverArt, CoverKind, Id3v2Revision, TagBlockContent, TagBlockKind, TagEngine,
    TagField, TrackFile,
};

/// `fLaC` + a non-last STREAMINFO block (34 bytes, all zeroed out except a
/// plausible sample rate/channels/bit depth) + a trailing PADDING block + one
/// audio frame.
///
/// STREAMINFO-only files (no padding) make lofty's FLAC writer index past
/// the metadata block when it tries to splice one in, since it assumes
/// there's always at least a padding block to reuse or resize — true of
/// every file a real encoder produces, but not of a hand-built minimal one.
/// Shaping the fixture the way real encoders do (metadata always followed by
/// padding) avoids that and exercises the same code path real files take.
const MINIMAL_FLAC: [u8; 76] = [
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
    // One audio frame (#416): 4096 samples of stereo silence, two CONSTANT
    // subframes, with their CRCs. A file is tags followed by audio, and a
    // tag write is only ever tested against that shape.
    0xff, 0xf8, 0xc9, 0x18, 0x00, 0xc2, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xb8, 0xee,
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

/// #612: the release country is one field under each format's own name for it.
/// On ID3v2 the standard frame showed as a field named after its description,
/// and the field the import and the mask fill went out as a `TXXX:RELEASECOUNTRY`
/// (an MP4 freeform atom) that nothing reads as the country.
#[test]
fn the_release_country_is_written_under_each_formats_standard_name() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{ExtendedTextFrame, Frame, Id3v2Tag};
    use lofty::iff::wav::{RiffInfoList, WavFile};
    use lofty::mp4::{Atom, AtomData, AtomIdent};
    use lofty::mpeg::MpegFile;
    use lofty::prelude::TagExt;
    use lofty::TextEncoding;

    let country = TagField::Custom("RELEASECOUNTRY".to_string());
    let standard = "MusicBrainz Album Release Country";
    let user_text = |path: &PathBuf| -> Vec<(String, String)> {
        let mut file = std::fs::File::open(path).unwrap();
        let mpeg = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
        let mut frames: Vec<(String, String)> = mpeg
            .id3v2()
            .map(|tag| {
                tag.iter()
                    .filter_map(|frame| match frame {
                        Frame::UserText(user) => {
                            Some((user.description.to_string(), user.content.to_string()))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        frames.sort();
        frames
    };
    let txxx = |description: &str, content: &str| {
        Frame::UserText(ExtendedTextFrame::new(
            TextEncoding::UTF8,
            description.to_string(),
            content.to_string(),
        ))
    };

    // MP3 with an older version's frame beside the standard one: the standard
    // value is the field, and a save leaves only the standard frame.
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-country-both-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");
    let mut seeded = Id3v2Tag::new();
    seeded.insert(txxx("RELEASECOUNTRY", "DE"));
    seeded.insert(txxx(standard, "FR"));
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();
    let read = TagEngine::read(&path).expect("read tags");
    assert_eq!(read.tags.get(&country).map(String::as_str), Some("FR"));
    assert_eq!(
        read.tags.get(&TagField::Custom(standard.to_string())),
        None,
        "the standard frame shows as a field of its own"
    );
    TagEngine::write(&read).expect("write tags");
    let frames = user_text(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(frames, vec![(standard.to_string(), "FR".to_string())]);

    // MP3 with only the older frame: its value moves to the standard name.
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-country-old-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");
    let mut seeded = Id3v2Tag::new();
    seeded.insert(txxx("RELEASECOUNTRY", "DE"));
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();
    let read = TagEngine::read(&path).expect("read tags");
    assert_eq!(read.tags.get(&country).map(String::as_str), Some("DE"));
    TagEngine::write(&read).expect("write tags");
    let frames = user_text(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(frames, vec![(standard.to_string(), "DE".to_string())]);

    // M4A with a freeform `COUNTRY` atom: read as the country, saved under the
    // standard atom only.
    let path = temp_m4a_path("country");
    std::fs::write(&path, MINIMAL_M4A).expect("write fixture");
    let mut ilst = mp4_ilst(&path);
    ilst.insert(Atom::new(
        AtomIdent::Freeform {
            mean: "com.apple.iTunes".into(),
            name: "COUNTRY".into(),
        },
        AtomData::UTF8("JP".into()),
    ));
    ilst.save_to_path(&path, WriteOptions::default()).unwrap();
    let read = TagEngine::read(&path).expect("read tags");
    assert_eq!(read.tags.get(&country).map(String::as_str), Some("JP"));
    TagEngine::write(&read).expect("write tags");
    let atoms = mp4_freeform(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(
        atoms,
        vec![(
            "com.apple.iTunes".to_string(),
            standard.to_string(),
            vec!["JP".to_string()]
        )]
    );

    // WAV with a RIFF INFO `ICNT`: an edit of the country reaches it.
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-country-{}.wav",
        std::process::id()
    ));
    std::fs::write(&path, minimal_wav()).expect("write fixture");
    let mut info = RiffInfoList::new();
    info.insert("INAM".to_string(), "Title".to_string());
    info.insert("ICNT".to_string(), "DE".to_string());
    info.save_to_path(&path, WriteOptions::default()).unwrap();
    let mut read = TagEngine::read(&path).expect("read tags");
    assert_eq!(read.tags.get(&country).map(String::as_str), Some("DE"));
    read.tags.insert(country.clone(), "FR".to_string());
    TagEngine::write(&read).expect("write tags");
    let icnt = {
        let mut file = std::fs::File::open(&path).unwrap();
        WavFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .riff_info()
            .and_then(|list| {
                list.into_iter()
                    .find(|(id, _)| id.as_str() == "ICNT")
                    .map(|(_, value)| value.trim_end_matches('\0').to_string())
            })
    };
    std::fs::remove_file(&path).ok();
    assert_eq!(icnt.as_deref(), Some("FR"));
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

    let read = TagEngine::read_with_props(&path).expect("read");
    let (track, props, blocks) = (read.file, read.props, read.blocks);
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
    // And the same probe says which tag block it all came out of (#47).
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].kind, TagBlockKind::VorbisComments);
    assert!(blocks[0].read_from);
    assert_eq!(blocks[0].label(), "Vorbis Comments");
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

/// A cover write leaves the custom fields alone (#606): converting Vorbis
/// Comments to the generic tag drops every item it has no key for, and a cover
/// write that saved that tag deleted them.
#[test]
fn a_cover_write_keeps_the_custom_fields() {
    let path = temp_flac_path("cover-custom");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");
    let custom = TagField::Custom("TAGREX_CUSTOM_TEST".to_string());
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Title, "Title".to_string());
    tags.insert(custom.clone(), "Kept".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags,
    })
    .unwrap();
    let cover = CoverArt {
        mime: "image/png".to_string(),
        data: vec![0x89, 0x50, 0x4e, 0x47, 1, 2, 3],
        ..CoverArt::default()
    };

    TagEngine::write_covers(&path, std::slice::from_ref(&cover)).unwrap();
    let read = TagEngine::read(&path).unwrap();
    assert_eq!(read.tags.get(&custom).map(String::as_str), Some("Kept"));
    assert_eq!(
        read.tags.get(&TagField::Title).map(String::as_str),
        Some("Title")
    );
    assert_eq!(TagEngine::read_covers(&path).unwrap(), vec![cover]);

    TagEngine::write_covers(&path, &[]).unwrap();
    let read = TagEngine::read(&path).unwrap();
    assert_eq!(read.tags.get(&custom).map(String::as_str), Some("Kept"));
    assert!(TagEngine::read_covers(&path).unwrap().is_empty());

    std::fs::remove_file(&path).ok();
}

/// A tenth of a second of silence as WavPack (#608): the smallest file whose
/// tags live in an APE block.
const MINIMAL_WAVPACK: [u8; 54] = [
    0x77, 0x76, 0x70, 0x6b, 0x2e, 0x00, 0x00, 0x00, 0x10, 0x04, 0x00, 0x00, 0x20, 0x03, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x20, 0x03, 0x00, 0x00, 0x04, 0x18, 0x80, 0x00, 0x7f, 0xe5, 0xe8, 0xf3,
    0x02, 0x00, 0x03, 0x00, 0x04, 0x00, 0x05, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x8a, 0x02,
    0x00, 0x00, 0xff, 0x03, 0x09, 0x00,
];

/// An APE tag keeps its pictures as binary items the backend never hands over
/// as pictures (#608): they are read, written, replaced and removed here, and
/// the rest of the tag is left alone.
#[test]
fn ape_pictures_are_read_written_and_removed() {
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-test-ape-cover-{}.wv",
        std::process::id()
    ));
    std::fs::write(&path, MINIMAL_WAVPACK).expect("write fixture");
    let custom = TagField::Custom("TAGREX_CUSTOM_TEST".to_string());
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Title, "Title".to_string());
    tags.insert(custom.clone(), "Kept".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::WavPack,
        tags,
    })
    .unwrap();
    // The backend names the format from the bytes, so they have to be an image.
    let png = |tail: u8| vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, tail];
    let front = CoverArt {
        mime: "image/png".to_string(),
        data: png(1),
        ..CoverArt::default()
    };
    let back = CoverArt {
        mime: "image/png".to_string(),
        data: png(2),
        kind: CoverKind::Back,
        description: "Back".to_string(),
    };

    TagEngine::write_covers(&path, &[front.clone(), back.clone()]).unwrap();
    assert_eq!(
        TagEngine::read_covers(&path).unwrap(),
        vec![front.clone(), back]
    );
    let read = TagEngine::read_with_props(&path).unwrap();
    assert!(read.has_cover);
    assert_eq!(
        read.file.tags.get(&custom).map(String::as_str),
        Some("Kept")
    );
    assert_eq!(
        TagEngine::read_block(&path, TagBlockKind::Ape)
            .unwrap()
            .unwrap()
            .covers
            .len(),
        2
    );

    TagEngine::write_covers(&path, std::slice::from_ref(&front)).unwrap();
    assert_eq!(TagEngine::read_covers(&path).unwrap(), vec![front]);

    TagEngine::write_covers(&path, &[]).unwrap();
    let read = TagEngine::read_with_props(&path).unwrap();
    assert!(!read.has_cover);
    assert!(TagEngine::read_covers(&path).unwrap().is_empty());
    assert_eq!(
        read.file.tags.get(&TagField::Title).map(String::as_str),
        Some("Title")
    );
    assert_eq!(
        read.file.tags.get(&custom).map(String::as_str),
        Some("Kept")
    );

    std::fs::remove_file(&path).ok();
}

/// An MP3's APE block keeps its pictures through a block write and read, the
/// path a block conversion and its undo take (#608).
#[test]
fn an_mp3_ape_block_keeps_its_pictures() {
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-test-mp3-ape-cover-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");
    let mut content = TagBlockContent::default();
    content.tags.insert(TagField::Artist, "Someone".to_string());
    content.covers.push(CoverArt {
        mime: "image/png".to_string(),
        data: vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 7],
        ..CoverArt::default()
    });

    TagEngine::write_block(&path, TagBlockKind::Ape, None, &content).unwrap();

    assert_eq!(
        TagEngine::read_block(&path, TagBlockKind::Ape).unwrap(),
        Some(content)
    );
    std::fs::remove_file(&path).ok();
}

/// The listing's cover flag (#447) comes from the same probe as the tags, and
/// follows the picture through an embed and a removal.
#[test]
fn listing_probe_reports_whether_a_file_carries_a_cover() {
    let cover = CoverArt {
        mime: "image/png".to_string(),
        data: vec![0x89, 0x50, 0x4e, 0x47, 1, 2, 3, 4, 5],
        ..CoverArt::default()
    };

    let flac = temp_flac_path("has-cover");
    std::fs::write(&flac, MINIMAL_FLAC).expect("write fixture");
    let mp3 = std::env::temp_dir().join(format!("tagrex-has-cover-{}.mp3", std::process::id()));
    std::fs::write(&mp3, minimal_mp3()).expect("write fixture");

    for path in [&flac, &mp3] {
        assert!(!TagEngine::read_with_props(path).unwrap().has_cover);
        TagEngine::write_covers(path, std::slice::from_ref(&cover)).unwrap();
        assert!(
            TagEngine::read_with_props(path).unwrap().has_cover,
            "{} should report its cover",
            path.display()
        );
        TagEngine::write_covers(path, &[]).unwrap();
        assert!(!TagEngine::read_with_props(path).unwrap().has_cover);
        std::fs::remove_file(path).ok();
    }
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
                lofty::id3::v2::Frame::Private(p) => Some(p.private_data.to_vec()),
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
                Frame::Private(p) => Some(p.private_data.to_vec()),
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
            lofty::id3::v2::Frame::Text(text) => text.value.to_string(),
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
            Frame::Private(p) => Some(p.private_data.to_vec()),
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

/// #653: a file with an ID3v2 tag reads from that alone, so an album, a track
/// number or a genre only its ID3v1 tag states never reached the model, and
/// the #194 rewrite deleted them on a save that changed nothing.
#[test]
fn an_id3v1_field_the_id3v2_tag_lacks_survives_a_save() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v1::Id3v1Tag;
    use lofty::id3::v2::Id3v2Tag;
    use lofty::mpeg::MpegFile;
    use lofty::prelude::{Accessor, TagExt};

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-id3v1-beside-id3v2-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");
    let mut id3v2 = Id3v2Tag::new();
    id3v2.set_title("Title".to_string());
    id3v2
        .save_to_path(&path, WriteOptions::default())
        .expect("seed id3v2");
    Id3v1Tag {
        title: Some("Old title".to_string()),
        album: Some("Only in v1".to_string()),
        track_number: Some(7),
        genre: Some(12),
        ..Default::default()
    }
    .save_to_path(&path, WriteOptions::default())
    .expect("seed id3v1");

    let mut file = TagEngine::read(&path).expect("read");
    assert_eq!(
        file.tags.get(&TagField::Album),
        None,
        "read from ID3v2 alone"
    );
    file.tags.insert(TagField::Title, "New title".to_string());
    TagEngine::write(&file).expect("write");
    let mut handle = std::fs::File::open(&path).unwrap();
    let id3v1 = MpegFile::read_from(&mut handle, ParseOptions::new())
        .unwrap()
        .id3v1()
        .cloned()
        .expect("ID3v1 kept");
    std::fs::remove_file(&path).ok();

    assert_eq!(id3v1.title.as_deref(), Some("New title"));
    assert_eq!(id3v1.album.as_deref(), Some("Only in v1"));
    assert_eq!(id3v1.track_number, Some(7));
    assert_eq!(id3v1.genre, Some(12));
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

/// #197: what encoded the file, how long it runs and what kind of file it is
/// are provenance, not metadata. They used to read back as ordinary editable
/// custom fields, so a clear listed them as cleared — and every write decided
/// their fate. Now the model neither shows them nor touches them.
#[test]
fn provenance_frames_are_neither_shown_nor_written_over() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, FrameId, Id3v2Tag, TextInformationFrame};
    use lofty::mpeg::MpegFile;
    use lofty::prelude::{Accessor, TagExt};
    use lofty::TextEncoding;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-provenance-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");

    let mut seeded = Id3v2Tag::new();
    seeded.set_artist("Original".to_string());
    for id in ["TSSE", "TENC", "TLEN", "TFLT"] {
        seeded.insert(Frame::Text(TextInformationFrame::new(
            FrameId::Valid(id.into()),
            TextEncoding::UTF8,
            format!("v-{id}"),
        )));
    }
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();

    let frame_ids = |path: &PathBuf| -> Vec<String> {
        let mut file = std::fs::File::open(path).unwrap();
        let mpeg = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
        mpeg.id3v2()
            .map(|tag| {
                tag.clone()
                    .into_iter()
                    .map(|frame| frame.id().as_str().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };

    // Not fields: nothing to show in the table, nothing for a clear to claim.
    let track = TagEngine::read(&path).expect("read tags");
    for name in ["EncoderSettings", "EncodedBy", "Length", "FileType"] {
        assert!(
            !track.tags.contains_key(&TagField::Custom(name.to_string())),
            "{name} came back as an editable field"
        );
    }

    // And a clear — the emptiest write there is — leaves all four alone.
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Mp3,
        tags: BTreeMap::new(),
    })
    .expect("clear tags");

    let after = frame_ids(&path);
    std::fs::remove_file(&path).ok();
    for id in ["TSSE", "TENC", "TLEN", "TFLT"] {
        assert!(after.contains(&id.to_string()), "the clear destroyed {id}");
    }
    assert!(
        !after.contains(&"TPE1".to_string()),
        "the clear kept the artist it was supposed to remove"
    );
}

/// The same promise on the generic writer, which every non-ID3v2 format uses:
/// its retain reads "not in the model" as "delete", so provenance needs the
/// same exemption there (#197).
#[test]
fn provenance_survives_the_generic_writer_too() {
    use lofty::config::WriteOptions;
    use lofty::prelude::{ItemKey, TagExt};
    use lofty::tag::{Tag, TagItem, TagType};

    let path = temp_flac_path("provenance");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");

    let mut seeded = Tag::new(TagType::VorbisComments);
    seeded.push_unchecked(TagItem::new(
        ItemKey::TrackArtist,
        lofty::tag::ItemValue::Text("Original".to_string()),
    ));
    seeded.push_unchecked(TagItem::new(
        ItemKey::EncoderSettings,
        lofty::tag::ItemValue::Text("Lavf52.78.4".to_string()),
    ));
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();

    assert!(
        !TagEngine::read(&path)
            .expect("read tags")
            .tags
            .contains_key(&TagField::Custom("EncoderSettings".to_string())),
        "the encoder setting came back as an editable field"
    );

    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags: BTreeMap::new(),
    })
    .expect("clear tags");

    let kept = {
        use lofty::file::TaggedFileExt;
        let tagged = lofty::probe::Probe::open(&path).unwrap().read().unwrap();
        tagged
            .primary_tag()
            .and_then(|tag| tag.get_string(ItemKey::EncoderSettings).map(str::to_string))
    };
    std::fs::remove_file(&path).ok();
    assert_eq!(kept.as_deref(), Some("Lavf52.78.4"));
}

/// #201: the backend stopped representing a format-specific item in its generic
/// tag, so a custom field has to be put on and taken off the concrete tag. On
/// ID3v2 that is a `TXXX` frame, and it has to carry the name the field has —
/// the whole point of a custom field is that the file, not the model, names it.
///
/// The second write is the other half: a custom field the model no longer holds
/// has to go, or an edit would only ever add names to a file.
#[test]
fn a_custom_field_is_a_user_text_frame_named_after_the_field() {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::id3::v2::Frame;
    use lofty::mpeg::MpegFile;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-custom-id3v2-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");

    let user_text = |path: &PathBuf| -> Vec<(String, String)> {
        let mut file = std::fs::File::open(path).unwrap();
        let mpeg = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
        mpeg.id3v2()
            .map(|tag| {
                tag.iter()
                    .filter_map(|frame| match frame {
                        Frame::UserText(user) => {
                            Some((user.description.to_string(), user.content.to_string()))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    };

    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Artist".to_string());
    tags.insert(
        TagField::Custom("TAGREX_CUSTOM_TEST".to_string()),
        "Energetic".to_string(),
    );
    tags.insert(
        TagField::Custom("RELEASECOUNTRY".to_string()),
        "DE".to_string(),
    );
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Mp3,
        tags: tags.clone(),
    })
    .expect("write tags");

    let mut written = user_text(&path);
    written.sort();
    // The release country has a standard TXXX name of its own on ID3v2 (#612).
    assert_eq!(
        written,
        vec![
            (
                "MusicBrainz Album Release Country".to_string(),
                "DE".to_string()
            ),
            ("TAGREX_CUSTOM_TEST".to_string(), "Energetic".to_string()),
        ],
        "the custom fields did not land in TXXX frames of their own names"
    );

    let read_back = TagEngine::read(&path).expect("read tags");
    assert_eq!(
        read_back
            .tags
            .get(&TagField::Custom("TAGREX_CUSTOM_TEST".to_string()))
            .map(String::as_str),
        Some("Energetic")
    );
    assert_eq!(
        read_back
            .tags
            .get(&TagField::Custom("RELEASECOUNTRY".to_string()))
            .map(String::as_str),
        Some("DE")
    );

    // Drop one of them and write again: it has to leave the file.
    tags.remove(&TagField::Custom("RELEASECOUNTRY".to_string()));
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Mp3,
        tags,
    })
    .expect("write tags again");

    let remaining = user_text(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(
        remaining,
        vec![("TAGREX_CUSTOM_TEST".to_string(), "Energetic".to_string())],
        "a custom field the model dropped stayed in the file"
    );
}

/// The same thing on Vorbis Comments, where the danger is the opposite one
/// (#201): the backend's generic tag does not carry an unknown comment across
/// the conversion at all, so a save built from it alone would silently *delete*
/// every custom field the file had — including ones this app never touched.
#[test]
fn an_unrelated_edit_leaves_another_taggers_comments_alone() {
    use lofty::config::WriteOptions;
    use lofty::ogg::tag::VorbisComments;
    use lofty::prelude::TagExt;

    let path = temp_flac_path("foreign-comments");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");

    let mut seeded = VorbisComments::default();
    seeded.push("ARTIST".to_string(), "Original".to_string());
    seeded.push("SOMEONE_ELSES_FIELD".to_string(), "keep me".to_string());
    seeded.push("REPLAYGAIN_TRACK_GAIN".to_string(), "-6.30 dB".to_string());
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();

    // Both are fields, under the names the file spells them with.
    let read = TagEngine::read(&path).expect("read tags");
    assert_eq!(
        read.tags
            .get(&TagField::Custom("SOMEONE_ELSES_FIELD".to_string()))
            .map(String::as_str),
        Some("keep me")
    );
    assert_eq!(
        read.tags
            .get(&TagField::Custom("REPLAYGAIN_TRACK_GAIN".to_string()))
            .map(String::as_str),
        Some("-6.30 dB"),
        "a key the backend knows but the model doesn't must keep the file's spelling"
    );

    // Edit only the artist, the way an ordinary apply would.
    let mut tags = read.tags.clone();
    tags.insert(TagField::Artist, "Edited".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags,
    })
    .expect("write tags");

    let comments: Vec<(String, String)> = {
        use lofty::config::ParseOptions;
        use lofty::file::AudioFile;
        use lofty::flac::FlacFile;
        let mut file = std::fs::File::open(&path).unwrap();
        FlacFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .vorbis_comments()
            .map(|tag| {
                tag.items()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    std::fs::remove_file(&path).ok();

    assert!(
        comments.contains(&("SOMEONE_ELSES_FIELD".to_string(), "keep me".to_string())),
        "an unrelated edit destroyed another tagger's comment, got {comments:?}"
    );
    assert!(
        comments.contains(&("REPLAYGAIN_TRACK_GAIN".to_string(), "-6.30 dB".to_string())),
        "the ReplayGain value was rewritten under a different name, got {comments:?}"
    );
    assert!(
        comments.contains(&("ARTIST".to_string(), "Edited".to_string())),
        "the edit itself did not land, got {comments:?}"
    );
}

/// #47: a file can carry more than one tag block, and which one the app reads is
/// the difference between "the edit didn't take" and "you are looking at the
/// other answer" — the confusion behind #194. The report says so per file, off
/// the probe the listing already does.
#[test]
fn the_report_names_every_block_and_marks_the_one_being_read() {
    use lofty::config::WriteOptions;
    use lofty::id3::v1::Id3v1Tag;
    use lofty::prelude::{Accessor, TagExt};

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-blocks-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");

    // One block to start with, written as ID3v2.3 so the revision is not the
    // default and a wrong answer would show.
    let mut modern = lofty::id3::v2::Id3v2Tag::new();
    modern.set_artist("Current".to_string());
    modern
        .save_to_path(&path, WriteOptions::default().use_id3v23(true))
        .expect("seed id3v2");

    let read = TagEngine::read_with_props(&path).expect("read");
    assert_eq!(read.blocks.len(), 1);
    assert_eq!(read.blocks[0].kind, TagBlockKind::Id3v2);
    assert_eq!(read.blocks[0].label(), "ID3v2");
    assert!(read.blocks[0].read_from);
    // The revision is asked for separately, and only then is it exact — the
    // listing does not pay for it (#47).
    assert_eq!(
        TagEngine::id3v2_revision(&path).expect("revision"),
        Some(Id3v2Revision::V3)
    );

    // Now give it a legacy block beside the modern one.
    let legacy = Id3v1Tag {
        artist: Some("Stale".to_string()),
        ..Default::default()
    };
    legacy
        .save_to_path(&path, WriteOptions::default())
        .expect("seed id3v1");

    let read = TagEngine::read_with_props(&path).expect("read");
    std::fs::remove_file(&path).ok();

    let labels: Vec<&str> = read.blocks.iter().map(|b| b.label()).collect();
    assert!(
        labels.contains(&"ID3v2") && labels.contains(&"ID3v1"),
        "both blocks should be reported, got {labels:?}"
    );
    // Exactly one is the one being read, and it is the modern one — which is
    // also where a write goes, so the report and the writer agree.
    let read_from: Vec<&str> = read
        .blocks
        .iter()
        .filter(|b| b.read_from)
        .map(|b| b.kind.name())
        .collect();
    assert_eq!(read_from, vec!["ID3v2"]);
    // And the values on show came from it, not from the legacy block.
    assert_eq!(
        read.file.tags.get(&TagField::Artist).map(String::as_str),
        Some("Current")
    );
}

/// A hand-built ID3v2.3 tag, since both fixtures below need frames the backend
/// will not write. `frames` is the already-encoded frame block.
fn id3v23_tag(frames: &[u8]) -> Vec<u8> {
    let mut tag = vec![b'I', b'D', b'3', 3, 0, 0];
    // The tag header's size is synchsafe: seven bits per byte.
    let size = frames.len() as u32;
    tag.extend_from_slice(&[
        ((size >> 21) & 0x7f) as u8,
        ((size >> 14) & 0x7f) as u8,
        ((size >> 7) & 0x7f) as u8,
        (size & 0x7f) as u8,
    ]);
    tag.extend_from_slice(frames);
    tag
}

/// One ID3v2.3 frame. Frame sizes in 2.3 are plain big-endian, unlike the
/// header's.
fn id3v23_frame(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut frame = id.to_vec();
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(body);
    frame
}

/// #204, cause one: a `UFID` frame whose owner field is empty.
///
/// The backend errors on it in relaxed mode and substitutes an empty string in
/// best-attempt — the two modes forgive different things, and #183 picked
/// relaxed for its leniency about exactly this class of problem. A file in the
/// user's library was unreadable for it.
#[test]
fn a_frame_only_one_parsing_mode_forgives_does_not_cost_the_file() {
    let mut frames = Vec::new();
    // Empty owner: just the terminator, then the identifier.
    frames.extend_from_slice(&id3v23_frame(b"UFID", &[0x00, b'a', b'b', b'c']));
    // Latin-1 encoding byte, then the text.
    frames.extend_from_slice(&id3v23_frame(b"TIT2", b"\x00Comin On Strong"));

    let path =
        std::env::temp_dir().join(format!("tagrex-tag-engine-ufid-{}.mp3", std::process::id()));
    let mut data = id3v23_tag(&frames);
    data.extend_from_slice(&minimal_mp3());
    std::fs::write(&path, data).expect("write fixture");

    let read = TagEngine::read(&path);
    std::fs::remove_file(&path).ok();
    let read = read.expect("the file must still be readable");
    assert_eq!(
        read.tags.get(&TagField::Title).map(String::as_str),
        Some("Comin On Strong")
    );
}

/// #204, cause two: the audio starts past the window the backend sniffs, and
/// the junk in between holds a stray sync of a different format.
///
/// The real file was read as AAC — 1035 bytes of junk with an ADTS-looking
/// `FF F1` inside it — and then failed as AAC, because it is an MP3. Its title,
/// musical key and tempo were all there to be had.
#[test]
fn audio_hiding_behind_junk_is_still_found_by_the_extension() {
    let frames = id3v23_frame(b"TIT2", b"\x00Weekend Lover");
    let path =
        std::env::temp_dir().join(format!("tagrex-tag-engine-junk-{}.mp3", std::process::id()));

    let mut data = id3v23_tag(&frames);
    // More junk than the backend's 1024-byte tolerance, with a false ADTS sync
    // early enough for the sniff to find it and conclude AAC.
    let mut junk = vec![0x00u8; 1035];
    junk[400] = 0xFF;
    junk[401] = 0xF1;
    data.extend_from_slice(&junk);
    data.extend_from_slice(&minimal_mp3());
    std::fs::write(&path, data).expect("write fixture");

    let read = TagEngine::read(&path);
    std::fs::remove_file(&path).ok();
    let read = read.expect("the file must still be readable");
    assert_eq!(read.format, AudioFormat::Mp3, "read as the wrong container");
    assert_eq!(
        read.tags.get(&TagField::Title).map(String::as_str),
        Some("Weekend Lover")
    );
}

/// The ladder must not turn a file that is genuinely not audio into one that
/// reads: falling back is for a file the backend can parse, not for anything
/// wearing the right extension.
#[test]
fn a_file_that_is_not_audio_at_all_still_fails() {
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-notaudio-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, b"this is a text file that happens to be named .mp3").unwrap();
    let read = TagEngine::read(&path);
    std::fs::remove_file(&path).ok();
    assert!(read.is_err(), "a text file must not read as audio");
}

/// #648: an MP3 whose ID3v2 tag opens with an extended header, built by hand
/// because the backend never writes one unprompted. `extended` is the extended
/// header as it sits on disk, `frames` the frame block, `footer` adds a 2.4
/// footer.
fn mp3_with_extended_header(
    name: &str,
    version: u8,
    unsynchronised: bool,
    extended: &[u8],
    frames: &[u8],
    footer: bool,
) -> PathBuf {
    let mut flags = 0x40;
    if unsynchronised {
        flags |= 0x80;
    }
    if footer {
        flags |= 0x10;
    }
    let size = (extended.len() + frames.len()) as u32;
    let size = [
        ((size >> 21) & 0x7f) as u8,
        ((size >> 14) & 0x7f) as u8,
        ((size >> 7) & 0x7f) as u8,
        (size & 0x7f) as u8,
    ];
    let mut data = vec![b'I', b'D', b'3', version, 0, flags];
    data.extend_from_slice(&size);
    data.extend_from_slice(extended);
    data.extend_from_slice(frames);
    if footer {
        data.extend_from_slice(&[b'3', b'D', b'I', version, 0, flags]);
        data.extend_from_slice(&size);
    }
    data.extend_from_slice(&minimal_mp3());
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-exthdr-{name}-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, data).expect("write fixture");
    path
}

/// The bytes after the file's leading ID3v2 tag (and its footer, if any).
fn after_id3v2(path: &PathBuf) -> Vec<u8> {
    let data = std::fs::read(path).unwrap();
    if &data[..3] != b"ID3" {
        return data;
    }
    let size = data[6..10]
        .iter()
        .fold(0usize, |acc, byte| (acc << 7) | usize::from(byte & 0x7f));
    let footer = if data[5] & 0x10 != 0 { 10 } else { 0 };
    data[10 + size + footer..].to_vec()
}

/// Read the file, retitle it, write it, and return what it reads as after.
fn retitle(path: &std::path::Path, title: &str) -> TrackFile {
    let mut file = TagEngine::read(path).expect("read");
    file.tags.insert(TagField::Title, title.to_string());
    TagEngine::write(&file).expect("write");
    TagEngine::read(path).expect("read back")
}

/// #648: a 2.4 tag with an extended header read fine, but every save cut as
/// many bytes off the start of the audio as the extended header was long.
#[test]
fn an_id3v24_extended_header_costs_the_audio_nothing() {
    // Size 6 (synchsafe, counting itself), one flag byte, no flags set.
    let extended = [0, 0, 0, 6, 1, 0];
    let mut frames = b"TIT2".to_vec();
    frames.extend_from_slice(&[0, 0, 0, 6, 0, 0, 0]);
    frames.extend_from_slice(b"Hello");
    frames.extend_from_slice(&[0; 32]); // padding
    let path = mp3_with_extended_header("v24", 4, false, &extended, &frames, false);

    let before = TagEngine::read(&path).expect("read");
    assert_eq!(
        before.tags.get(&TagField::Title).map(String::as_str),
        Some("Hello")
    );
    let after = retitle(&path, "Changed");
    let audio = after_id3v2(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(
        after.tags.get(&TagField::Title).map(String::as_str),
        Some("Changed")
    );
    assert_eq!(audio, minimal_mp3(), "the save changed the audio");
}

/// #648: the same with a 2.4 footer, which repeats the header and so has to
/// lose the extended-header flag and the bytes along with it.
#[test]
fn an_id3v24_extended_header_with_a_footer_costs_the_audio_nothing() {
    // Restrictions set: size 8, one flag byte, the restrictions flag, its
    // length and the restrictions byte.
    let extended = [0, 0, 0, 8, 1, 0x10, 1, 0];
    let mut frames = b"TIT2".to_vec();
    frames.extend_from_slice(&[0, 0, 0, 6, 0, 0, 0]);
    frames.extend_from_slice(b"Hello");
    let path = mp3_with_extended_header("v24footer", 4, false, &extended, &frames, true);

    let before = TagEngine::read(&path).expect("read");
    assert_eq!(
        before.tags.get(&TagField::Title).map(String::as_str),
        Some("Hello")
    );
    let after = retitle(&path, "Changed");
    let audio = after_id3v2(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(
        after.tags.get(&TagField::Title).map(String::as_str),
        Some("Changed")
    );
    assert_eq!(audio, minimal_mp3(), "the save changed the audio");
}

/// #648: a 2.3 extended header was read with the 2.4 layout, so the frames
/// were taken for padding: the file read as untagged, and the save that
/// followed dropped every frame and cut the audio too.
#[test]
fn an_id3v23_extended_header_hides_no_frames() {
    // Size 6 (not counting itself), two flag bytes, a four-byte padding size.
    let extended = [0, 0, 0, 6, 0, 0, 0, 0, 0, 16];
    let mut frames = Vec::new();
    frames.extend_from_slice(&id3v23_frame(b"TIT2", b"\x00Hello"));
    frames.extend_from_slice(&id3v23_frame(b"TPE1", b"\x00Someone"));
    frames.extend_from_slice(&[0; 16]); // the padding the extended header counts
    let path = mp3_with_extended_header("v23", 3, false, &extended, &frames, false);

    let before = TagEngine::read(&path).expect("read");
    assert_eq!(
        before.tags.get(&TagField::Title).map(String::as_str),
        Some("Hello")
    );
    assert_eq!(
        before.tags.get(&TagField::Artist).map(String::as_str),
        Some("Someone")
    );
    let after = retitle(&path, "Changed");
    let audio = after_id3v2(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(
        after.tags.get(&TagField::Title).map(String::as_str),
        Some("Changed")
    );
    assert_eq!(
        after.tags.get(&TagField::Artist).map(String::as_str),
        Some("Someone")
    );
    assert_eq!(audio, minimal_mp3(), "the save changed the audio");
}

/// #648: a WAV's `ID3 ` chunk can hold a 2.3 tag with an extended header too,
/// and read as untagged the same way; the save that followed dropped the chunk.
#[test]
fn an_id3v23_extended_header_in_a_wav_chunk_hides_no_frames() {
    let mut tag = vec![b'I', b'D', b'3', 3, 0, 0x40];
    let mut body = vec![0, 0, 0, 6, 0, 0, 0, 0, 0, 0];
    body.extend_from_slice(&id3v23_frame(b"TIT2", b"\x00Hello"));
    body.extend_from_slice(&id3v23_frame(b"TPE1", b"\x00Someone"));
    tag.extend_from_slice(&[0, 0, 0, body.len() as u8]);
    tag.extend_from_slice(&body);
    if tag.len() % 2 == 1 {
        tag.push(0);
    }
    let mut data = minimal_wav();
    data.extend_from_slice(b"ID3 ");
    data.extend_from_slice(&(tag.len() as u32).to_le_bytes());
    data.extend_from_slice(&tag);
    let riff_size = (data.len() - 8) as u32;
    data[4..8].copy_from_slice(&riff_size.to_le_bytes());
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-exthdr-wav-{}.wav",
        std::process::id()
    ));
    std::fs::write(&path, data).expect("write fixture");

    let before = TagEngine::read(&path).expect("read");
    let after = retitle(&path, "Changed");
    let audio_kept = std::fs::read(&path)
        .unwrap()
        .windows(minimal_wav().len() - 12)
        .any(|window| window == &minimal_wav()[12..]);
    std::fs::remove_file(&path).ok();
    assert_eq!(
        before.tags.get(&TagField::Title).map(String::as_str),
        Some("Hello")
    );
    assert_eq!(
        after.tags.get(&TagField::Title).map(String::as_str),
        Some("Changed")
    );
    assert_eq!(
        after.tags.get(&TagField::Artist).map(String::as_str),
        Some("Someone")
    );
    assert!(audio_kept, "the save changed the audio");
}

/// #648: an unsynchronised 2.3 tag has its extended header unsynchronised as
/// well, so a CRC holding an `FF` is one byte longer on disk than its size says.
#[test]
fn an_unsynchronised_id3v23_extended_header_is_measured_on_disk() {
    // Size 10, the CRC flag, no padding, then a CRC whose first byte is FF and
    // so is followed by the 00 unsynchronisation inserts.
    let extended = [0, 0, 0, 10, 0x80, 0, 0, 0, 0, 0, 0xFF, 0x00, 1, 2, 3];
    let frames = id3v23_frame(b"TIT2", b"\x00Hello");
    let path = mp3_with_extended_header("v23unsync", 3, true, &extended, &frames, false);

    let before = TagEngine::read(&path).expect("read");
    assert_eq!(
        before.tags.get(&TagField::Title).map(String::as_str),
        Some("Hello")
    );
    let after = retitle(&path, "Changed");
    let audio = after_id3v2(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(
        after.tags.get(&TagField::Title).map(String::as_str),
        Some("Changed")
    );
    assert_eq!(audio, minimal_mp3(), "the save changed the audio");
}

/// #205: the ID3v2 revision a conversion asks for is the one that lands on
/// disk, whichever way the app-wide preference happens to be set.
#[test]
fn a_conversion_writes_the_id3v2_revision_it_was_asked_for() {
    use tagrex_core::model::{Id3v2Revision, TagBlockContent, TagBlockKind, TagField};

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-revision-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");

    let mut content = TagBlockContent::default();
    content
        .tags
        .insert(TagField::Artist, "Revision Test".to_string());

    for revision in [Id3v2Revision::V3, Id3v2Revision::V4] {
        TagEngine::write_block(&path, TagBlockKind::Id3v2, Some(revision), &content)
            .expect("write block");
        assert_eq!(
            TagEngine::id3v2_revision(&path).expect("read revision"),
            Some(revision),
            "asked for {} and the file says otherwise",
            revision.name()
        );
    }
    std::fs::remove_file(&path).ok();
}

/// #205: what a conversion costs is worked out against the backend's own
/// conversion, so the warning names what will really go — not a guess.
#[test]
fn conversion_loss_names_what_the_target_block_cannot_hold() {
    use tagrex_core::model::{TagBlockContent, TagBlockKind, TagField};

    let mut content = TagBlockContent::default();
    content.tags.insert(TagField::Artist, "Kept".to_string());
    content.tags.insert(TagField::Title, "Kept".to_string());
    content.tags.insert(TagField::Isrc, "Dropped".to_string());
    content.covers.push(CoverArt {
        mime: "image/png".to_string(),
        data: vec![1, 2, 3],
        ..CoverArt::default()
    });

    // ID3v2 holds everything here, pictures included.
    assert!(content.conversion_loss(TagBlockKind::Id3v2).is_empty());

    // ID3v1 has seven text slots and no room for a picture.
    let loss = content.conversion_loss(TagBlockKind::Id3v1);
    assert!(
        loss.fields.contains(&TagField::Isrc),
        "ID3v1 cannot hold an ISRC, got {:?}",
        loss.fields
    );
    assert!(
        !loss.fields.contains(&TagField::Artist),
        "the artist survives ID3v1"
    );
    assert_eq!(loss.pictures, 1, "ID3v1 cannot hold a picture");
}

/// #205: a container is only offered the blocks it can actually be given.
#[test]
fn only_blocks_a_container_can_carry_are_offered() {
    use tagrex_core::model::{AudioFormat, TagBlockKind};

    let mp3 = TagBlockKind::writable_for(AudioFormat::Mp3);
    assert!(mp3.contains(&TagBlockKind::Id3v2));
    assert!(mp3.contains(&TagBlockKind::Id3v1));
    assert!(
        !mp3.contains(&TagBlockKind::Mp4Ilst),
        "an MP3 cannot carry MP4 atoms"
    );

    let flac = TagBlockKind::writable_for(AudioFormat::Flac);
    assert!(flac.contains(&TagBlockKind::VorbisComments));
    assert!(
        !flac.contains(&TagBlockKind::Id3v1),
        "a FLAC cannot carry ID3v1"
    );
}

/// #47: removing one tag block must leave every other block in the file exactly
/// as it was, and the snapshot must put the removed one back.
///
/// ID3v1 is the case worth having exact — 81% of a real library carries one
/// beside its ID3v2, and it holds seven text fields and nothing else, so there
/// is nothing a rebuild can fail to bring back.
#[test]
fn removing_a_block_leaves_the_others_alone_and_undo_puts_it_back() {
    use lofty::config::WriteOptions;
    use lofty::id3::v1::Id3v1Tag;
    use lofty::prelude::{Accessor, TagExt};

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-block-remove-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");

    let mut modern = lofty::id3::v2::Id3v2Tag::new();
    modern.set_artist("Current".to_string());
    modern.set_title("Modern Title".to_string());
    modern
        .save_to_path(&path, WriteOptions::default())
        .expect("seed id3v2");
    let legacy = Id3v1Tag {
        artist: Some("Stale Artist".to_string()),
        title: Some("Stale Title".to_string()),
        ..Default::default()
    };
    legacy
        .save_to_path(&path, WriteOptions::default())
        .expect("seed id3v1");

    let kinds = |path: &PathBuf| -> Vec<TagBlockKind> {
        TagEngine::read_with_props(path)
            .expect("read")
            .blocks
            .iter()
            .map(|b| b.kind)
            .collect()
    };
    assert_eq!(kinds(&path).len(), 2, "seeding failed");

    // Snapshot, then remove.
    let snapshot = TagEngine::read_block(&path, TagBlockKind::Id3v1)
        .expect("read block")
        .expect("the block is there");
    assert!(
        TagBlockContent::exact(TagBlockKind::Id3v1),
        "ID3v1 has nothing a rebuild can miss"
    );
    TagEngine::remove_block(&path, TagBlockKind::Id3v1).expect("remove");

    assert_eq!(
        kinds(&path),
        vec![TagBlockKind::Id3v2],
        "the wrong block went"
    );
    // The block that stayed is untouched, values and all.
    let after = TagEngine::read(&path).expect("read");
    assert_eq!(
        after.tags.get(&TagField::Artist).map(String::as_str),
        Some("Current")
    );

    // Undo.
    TagEngine::restore_block(&path, TagBlockKind::Id3v1, &snapshot).expect("restore");
    let restored = TagEngine::read_block(&path, TagBlockKind::Id3v1)
        .expect("read block")
        .expect("the block is back");
    std::fs::remove_file(&path).ok();

    assert_eq!(restored, snapshot, "the block did not come back as it went");
    // And it really holds the legacy values, not an empty block that happens to
    // compare equal to an empty snapshot.
    assert_eq!(
        snapshot.tags.get(&TagField::Artist).map(String::as_str),
        Some("Stale Artist")
    );
    assert_eq!(
        restored.tags.get(&TagField::Title).map(String::as_str),
        Some("Stale Title")
    );
}

/// Every unsynchronized-lyrics frame in the file, as (language, description,
/// text). Lyrics have no field of their own — they reach the model through the
/// custom-field catch-all — so what has to be pinned is the frame count as much
/// as the value: a duplicate reads back fine and still leaves the file saying
/// the same thing twice (#264).
fn lyrics_frames(path: &PathBuf) -> Vec<(String, String, String)> {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::id3::v2::Frame;
    use lofty::mpeg::MpegFile;

    let mut file = std::fs::File::open(path).unwrap();
    let mp3 = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
    mp3.id3v2()
        .map(|tag| {
            tag.clone()
                .into_iter()
                .filter_map(|frame| match frame {
                    Frame::UnsynchronizedText(lyrics) => Some((
                        String::from_utf8_lossy(&lyrics.language).into_owned(),
                        lyrics.description.to_string(),
                        lyrics.content.to_string(),
                    )),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Seed an MP3 with one `USLT` frame and an artist, and hand back its path.
fn mp3_with_lyrics(name: &str, text: &str) -> PathBuf {
    use lofty::config::WriteOptions;
    use lofty::id3::v2::{Frame, Id3v2Tag, UnsynchronizedTextFrame};
    use lofty::prelude::{Accessor, TagExt};
    use lofty::TextEncoding;

    let path =
        std::env::temp_dir().join(format!("tagrex-lyrics-{name}-{}.mp3", std::process::id()));
    std::fs::write(&path, minimal_mp3()).unwrap();

    let mut seeded = Id3v2Tag::new();
    seeded.set_artist("Original".to_string());
    seeded.insert(Frame::UnsynchronizedText(UnsynchronizedTextFrame::new(
        TextEncoding::UTF8,
        *b"eng",
        String::new(),
        text.to_string(),
    )));
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();
    path
}

#[test]
fn writing_an_unrelated_field_leaves_the_lyrics_frame_alone() {
    let path = mp3_with_lyrics("untouched", "line one\nline two");
    assert_eq!(lyrics_frames(&path).len(), 1, "seeding failed");

    let mut file = TagEngine::read(&path).unwrap();
    file.tags.insert(TagField::Title, "New title".into());
    TagEngine::write(&file).unwrap();

    assert_eq!(
        lyrics_frames(&path),
        vec![(
            "eng".to_string(),
            String::new(),
            "line one\nline two".to_string()
        )],
        "a save that never touched the lyrics must leave exactly the frame the file had"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn edited_lyrics_reach_the_file_in_the_frame_it_had() {
    let path = mp3_with_lyrics("edited", "line one\nline two");

    let mut file = TagEngine::read(&path).unwrap();
    assert_eq!(
        file.tags.get(&TagField::Custom("USLT".to_string())),
        Some(&"line one\nline two".to_string()),
        "lyrics should read into the model as a custom field"
    );
    file.tags.insert(
        TagField::Custom("USLT".to_string()),
        "edited\nlyrics".into(),
    );
    TagEngine::write(&file).unwrap();

    assert_eq!(
        lyrics_frames(&path),
        vec![(
            "eng".to_string(),
            String::new(),
            "edited\nlyrics".to_string()
        )],
        "the edit belongs in the frame the value came from, keeping its language"
    );
    assert_eq!(
        TagEngine::read(&path)
            .unwrap()
            .tags
            .get(&TagField::Custom("USLT".to_string())),
        Some(&"edited\nlyrics".to_string())
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn repeated_saves_do_not_accumulate_lyrics_frames() {
    let path = mp3_with_lyrics("repeated", "line one");
    for _ in 0..3 {
        let file = TagEngine::read(&path).unwrap();
        TagEngine::write(&file).unwrap();
    }
    assert_eq!(lyrics_frames(&path).len(), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn lyrics_in_another_language_are_left_where_they_are() {
    use lofty::config::WriteOptions;
    use lofty::id3::v2::{Frame, Id3v2Tag, UnsynchronizedTextFrame};
    use lofty::prelude::TagExt;
    use lofty::TextEncoding;

    let path = std::env::temp_dir().join(format!("tagrex-lyrics-two-{}.mp3", std::process::id()));
    std::fs::write(&path, minimal_mp3()).unwrap();

    let mut seeded = Id3v2Tag::new();
    for (language, text) in [(b"deu", "zeile eins"), (b"eng", "line one")] {
        seeded.insert(Frame::UnsynchronizedText(UnsynchronizedTextFrame::new(
            TextEncoding::UTF8,
            *language,
            String::new(),
            text.to_string(),
        )));
    }
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();
    assert_eq!(lyrics_frames(&path).len(), 2, "seeding failed");

    // The model can hold one value per field, so it reads the last of the two —
    // the other is lyrics it never saw, and editing the one it has must not
    // take the other down with it.
    let mut file = TagEngine::read(&path).unwrap();
    assert_eq!(
        file.tags.get(&TagField::Custom("USLT".to_string())),
        Some(&"line one".to_string())
    );
    file.tags
        .insert(TagField::Custom("USLT".to_string()), "line two".into());
    TagEngine::write(&file).unwrap();

    let mut frames = lyrics_frames(&path);
    frames.sort();
    assert_eq!(
        frames,
        vec![
            ("deu".to_string(), String::new(), "zeile eins".to_string()),
            ("eng".to_string(), String::new(), "line two".to_string()),
        ]
    );
    let _ = std::fs::remove_file(&path);
}

/// Every comment frame in the file, as (language, description, text) (#644).
fn comment_frames(path: &PathBuf) -> Vec<(String, String, String)> {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::id3::v2::Frame;
    use lofty::mpeg::MpegFile;

    let mut file = std::fs::File::open(path).unwrap();
    let mp3 = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
    let mut frames: Vec<_> = mp3
        .id3v2()
        .map(|tag| {
            tag.into_iter()
                .filter_map(|frame| match frame {
                    Frame::Comment(comment) => Some((
                        String::from_utf8_lossy(&comment.language).into_owned(),
                        comment.description.to_string(),
                        comment.content.to_string(),
                    )),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    frames.sort();
    frames
}

/// Seed an MP3 with the given `COMM` frames (language, description, text).
fn mp3_with_comments(name: &str, comments: &[(&[u8; 3], &str, &str)]) -> PathBuf {
    use lofty::config::WriteOptions;
    use lofty::id3::v2::{CommentFrame, Frame, Id3v2Tag};
    use lofty::prelude::{Accessor, TagExt};
    use lofty::TextEncoding;

    let path = std::env::temp_dir().join(format!("tagrex-comm-{name}-{}.mp3", std::process::id()));
    std::fs::write(&path, minimal_mp3()).unwrap();

    let mut seeded = Id3v2Tag::new();
    seeded.set_artist("Original".to_string());
    for (language, description, text) in comments {
        seeded.insert(Frame::Comment(CommentFrame::new(
            TextEncoding::UTF8,
            **language,
            description.to_string(),
            text.to_string(),
        )));
    }
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();
    path
}

const ITUNES_GAPLESS: &str = " 00000000 00000210 0000099C 00000000000A0B54";
const ITUNES_SOUND_CHECK: &str = " 000002B4 000002A3 00002A9D 0000298D";

#[test]
fn itunes_comment_frames_survive_a_save_and_are_not_the_comment() {
    let path = mp3_with_comments(
        "itunes",
        &[
            (b"eng", "", "a real comment"),
            (b"eng", "iTunSMPB", ITUNES_GAPLESS),
            (b"eng", "iTunNORM", ITUNES_SOUND_CHECK),
        ],
    );
    assert_eq!(comment_frames(&path).len(), 3, "seeding failed");

    let mut file = TagEngine::read(&path).unwrap();
    assert_eq!(
        file.tags.get(&TagField::Comment).map(String::as_str),
        Some("a real comment"),
        "the comment is the frame without a description, not iTunes' data"
    );
    file.tags.insert(TagField::Comment, "edited".into());
    TagEngine::write(&file).unwrap();

    assert_eq!(
        comment_frames(&path),
        vec![
            ("eng".to_string(), String::new(), "edited".to_string()),
            (
                "eng".to_string(),
                "iTunNORM".to_string(),
                ITUNES_SOUND_CHECK.to_string()
            ),
            (
                "eng".to_string(),
                "iTunSMPB".to_string(),
                ITUNES_GAPLESS.to_string()
            ),
        ],
        "the edit lands in the comment's own frame, keeping its language, and the iTunes frames stay"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_file_with_only_itunes_comment_frames_has_no_comment() {
    let path = mp3_with_comments(
        "itunes-only",
        &[
            (b"eng", "iTunSMPB", ITUNES_GAPLESS),
            (b"eng", "iTunNORM", ITUNES_SOUND_CHECK),
        ],
    );
    let before = comment_frames(&path);

    let mut file = TagEngine::read(&path).unwrap();
    assert_eq!(file.tags.get(&TagField::Comment), None);
    file.tags.insert(TagField::Title, "New title".into());
    TagEngine::write(&file).unwrap();
    assert_eq!(
        comment_frames(&path),
        before,
        "an unrelated save keeps them"
    );

    // A comment typed into such a file is a new frame beside them.
    let mut file = TagEngine::read(&path).unwrap();
    file.tags.insert(TagField::Comment, "typed".into());
    TagEngine::write(&file).unwrap();
    let after = comment_frames(&path);
    assert_eq!(after.len(), 3, "{after:?}");
    assert!(after
        .iter()
        .any(|(_, description, text)| description.is_empty() && text == "typed"));
    assert_eq!(
        TagEngine::read(&path)
            .unwrap()
            .tags
            .get(&TagField::Comment)
            .map(String::as_str),
        Some("typed")
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_comment_keeps_its_language() {
    let path = mp3_with_comments("language", &[(b"deu", "", "Kommentar")]);
    for _ in 0..2 {
        let file = TagEngine::read(&path).unwrap();
        TagEngine::write(&file).unwrap();
    }
    assert_eq!(
        comment_frames(&path),
        vec![("deu".to_string(), String::new(), "Kommentar".to_string())]
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_user_text_frame_keeps_the_case_of_its_description() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{ExtendedTextFrame, Frame, Id3v2Tag};
    use lofty::mpeg::MpegFile;
    use lofty::prelude::{Accessor, TagExt};
    use lofty::TextEncoding;

    let user_text = |path: &PathBuf| -> Vec<(String, String)> {
        let mut file = std::fs::File::open(path).unwrap();
        let mpeg = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
        let mut frames: Vec<(String, String)> = mpeg
            .id3v2()
            .map(|tag| {
                tag.iter()
                    .filter_map(|frame| match frame {
                        Frame::UserText(user) => {
                            Some((user.description.to_string(), user.content.to_string()))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        frames.sort();
        frames
    };
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-txxx-case-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).unwrap();
    let mut seeded = Id3v2Tag::new();
    seeded.set_artist("Original".to_string());
    for (description, content) in [
        ("replaygain_track_gain", "-6.50 dB"),
        ("replaygain_track_peak", "0.988"),
    ] {
        seeded.insert(Frame::UserText(ExtendedTextFrame::new(
            TextEncoding::UTF8,
            description.to_string(),
            content.to_string(),
        )));
    }
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();
    let before = user_text(&path);

    // An unrelated edit leaves the frames as the file spelled them.
    let mut file = TagEngine::read(&path).unwrap();
    file.tags.insert(TagField::Title, "New title".into());
    TagEngine::write(&file).unwrap();
    assert_eq!(user_text(&path), before);

    // An edited value lands in the same frame.
    let mut file = TagEngine::read(&path).unwrap();
    let gain = TagField::Custom("REPLAYGAIN_TRACK_GAIN".to_string());
    assert_eq!(file.tags.get(&gain).map(String::as_str), Some("-6.50 dB"));
    file.tags.insert(gain, "-7.00 dB".into());
    TagEngine::write(&file).unwrap();
    assert_eq!(
        user_text(&path),
        vec![
            ("replaygain_track_gain".to_string(), "-7.00 dB".to_string()),
            ("replaygain_track_peak".to_string(), "0.988".to_string()),
        ]
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_vorbis_label_keeps_the_name_the_file_gave_it() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::flac::FlacFile;
    use lofty::ogg::tag::VorbisComments;
    use lofty::prelude::TagExt;

    let comments = |path: &PathBuf| -> Vec<(String, String)> {
        let mut file = std::fs::File::open(path).unwrap();
        FlacFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .vorbis_comments()
            .map(|tag| {
                tag.items()
                    .filter(|(name, _)| name.as_bytes()[0] != b'A')
                    .map(|(name, value)| (name.to_string(), value.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    for (name, case) in [
        ("ORGANIZATION", "organization"),
        ("LABEL", "label"),
        ("", "none"),
    ] {
        let path = temp_flac_path(&format!("label-{case}"));
        std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");
        let mut seeded = VorbisComments::default();
        seeded.push("ARTIST".to_string(), "Original".to_string());
        if !name.is_empty() {
            seeded.push(name.to_string(), "Some Label".to_string());
        }
        seeded.save_to_path(&path, WriteOptions::default()).unwrap();

        let mut file = TagEngine::read(&path).unwrap();
        if name.is_empty() {
            file.tags.insert(TagField::Publisher, "Some Label".into());
        } else {
            assert_eq!(
                file.tags.get(&TagField::Publisher).map(String::as_str),
                Some("Some Label")
            );
            file.tags.insert(TagField::Artist, "Edited".into());
        }
        TagEngine::write(&file).unwrap();
        let expected = if name.is_empty() { "PUBLISHER" } else { name };
        assert_eq!(
            comments(&path),
            vec![(expected.to_string(), "Some Label".to_string())],
            "{case}"
        );

        let mut file = TagEngine::read(&path).unwrap();
        file.tags.insert(TagField::Publisher, "Other Label".into());
        TagEngine::write(&file).unwrap();
        assert_eq!(
            comments(&path),
            vec![(expected.to_string(), "Other Label".to_string())],
            "{case}: an edit goes under the same name"
        );
        std::fs::remove_file(&path).ok();
    }
}

#[test]
fn a_vorbis_field_keeps_the_alternative_name_the_file_gave_it() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::flac::FlacFile;
    use lofty::ogg::tag::VorbisComments;
    use lofty::prelude::TagExt;

    let comments = |path: &PathBuf| -> Vec<(String, String)> {
        let mut file = std::fs::File::open(path).unwrap();
        let mut items: Vec<(String, String)> = FlacFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .vorbis_comments()
            .map(|tag| {
                tag.items()
                    .map(|(name, value)| (name.to_string(), value.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        items.sort();
        items
    };
    let path = temp_flac_path("alternative-names");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");
    let mut seeded = VorbisComments::default();
    for (name, value) in [
        ("ARTIST", "Original"),
        ("ALBUM ARTIST", "Various Artists"),
        ("TRACKNUMBER", "3"),
        ("TOTALTRACKS", "12"),
        ("DISCNUMBER", "1"),
        ("TOTALDISCS", "2"),
    ] {
        seeded.push(name.to_string(), value.to_string());
    }
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();
    let before = comments(&path);

    let mut file = TagEngine::read(&path).unwrap();
    assert_eq!(
        file.tags.get(&TagField::AlbumArtist).map(String::as_str),
        Some("Various Artists")
    );
    assert_eq!(
        file.tags.get(&TagField::TrackTotal).map(String::as_str),
        Some("12")
    );
    file.tags.insert(TagField::Title, "New title".into());
    TagEngine::write(&file).unwrap();
    let mut expected = before.clone();
    expected.push(("TITLE".to_string(), "New title".to_string()));
    expected.sort();
    assert_eq!(
        comments(&path),
        expected,
        "an unrelated edit renames nothing"
    );

    let mut file = TagEngine::read(&path).unwrap();
    file.tags.insert(TagField::AlbumArtist, "Someone".into());
    file.tags.insert(TagField::TrackTotal, "13".into());
    TagEngine::write(&file).unwrap();
    let after = comments(&path);
    assert!(after.contains(&("ALBUM ARTIST".to_string(), "Someone".to_string())));
    assert!(after.contains(&("TOTALTRACKS".to_string(), "13".to_string())));
    assert!(after
        .iter()
        .all(|(name, _)| name != "ALBUMARTIST" && name != "TRACKTOTAL"));
    std::fs::remove_file(&path).ok();
}

#[test]
fn vorbis_lyrics_round_trip_as_a_custom_field() {
    let path = temp_flac_path("lyrics");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");

    let mut file = TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags: BTreeMap::new(),
    };
    file.tags.insert(TagField::Artist, "Original".into());
    file.tags.insert(
        TagField::Custom("LYRICS".to_string()),
        "line one\nline two".into(),
    );
    TagEngine::write(&file).unwrap();

    let read = TagEngine::read(&path).unwrap();
    assert_eq!(
        read.tags.get(&TagField::Custom("LYRICS".to_string())),
        Some(&"line one\nline two".to_string())
    );

    let mut edited = read;
    edited
        .tags
        .insert(TagField::Custom("LYRICS".to_string()), "edited".into());
    TagEngine::write(&edited).unwrap();
    assert_eq!(
        TagEngine::read(&path)
            .unwrap()
            .tags
            .get(&TagField::Custom("LYRICS".to_string())),
        Some(&"edited".to_string()),
        "the Vorbis side has no carry loop to duplicate the value — pin that it stays that way"
    );
    let _ = std::fs::remove_file(&path);
}

/// The smallest M4A an encoder writes: a fraction of a second of AAC silence,
/// with no metadata of its own.
const MINIMAL_M4A: &[u8] = include_bytes!("fixtures/minimal.m4a");

fn temp_m4a_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "tagrex-tag-engine-test-{name}-{}.m4a",
        std::process::id()
    ));
    path
}

/// The `ilst` of an M4A, straight off the file.
fn mp4_ilst(path: &PathBuf) -> lofty::mp4::Ilst {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::mp4::Mp4File;
    let mut file = std::fs::File::open(path).unwrap();
    Mp4File::read_from(&mut file, ParseOptions::new())
        .unwrap()
        .ilst()
        .cloned()
        .unwrap_or_default()
}

/// Every freeform atom of an M4A as (mean, name, text values).
fn mp4_freeform(path: &PathBuf) -> Vec<(String, String, Vec<String>)> {
    use lofty::mp4::{AtomData, AtomIdent};
    mp4_ilst(path)
        .into_iter()
        .filter_map(|atom| {
            let AtomIdent::Freeform { mean, name } = atom.ident() else {
                return None;
            };
            let values = atom
                .data()
                .filter_map(|data| match data {
                    AtomData::UTF8(text) => Some(text.clone()),
                    _ => None,
                })
                .collect();
            Some((mean.to_string(), name.to_string(), values))
        })
        .collect()
}

/// #609: a freeform atom can carry several values, and another tagger can file
/// one under its own namespace. The read showed only the first value, and any
/// edit wrote that one back under the default namespace, so the rest of the
/// values and the namespace were gone after changing the title.
#[test]
fn an_mp4_freeform_atom_keeps_every_value_and_its_namespace() {
    use lofty::config::WriteOptions;
    use lofty::mp4::{Atom, AtomData, AtomIdent};
    use lofty::prelude::TagExt;

    let path = temp_m4a_path("freeform");
    std::fs::write(&path, MINIMAL_M4A).expect("write fixture");
    let mut ilst = mp4_ilst(&path);
    let mut multi = Atom::new(
        AtomIdent::Freeform {
            mean: "com.apple.iTunes".into(),
            name: "MYMULTI".into(),
        },
        AtomData::UTF8("one".into()),
    );
    multi.push_data(AtomData::UTF8("two".into()));
    ilst.insert(multi);
    ilst.insert(Atom::new(
        AtomIdent::Freeform {
            mean: "org.example".into(),
            name: "ELSEWHERE".into(),
        },
        AtomData::UTF8("kept".into()),
    ));
    ilst.save_to_path(&path, WriteOptions::default()).unwrap();

    let read = TagEngine::read(&path).expect("read tags");
    let multi_field = TagField::Custom("MYMULTI".to_string());
    let elsewhere_field = TagField::Custom("ELSEWHERE".to_string());
    assert_eq!(
        read.tags.get(&multi_field).map(String::as_str),
        Some("one; two"),
        "every value of the atom is the file's"
    );

    // An unrelated edit puts both atoms back the way they were.
    let mut tags = read.tags.clone();
    tags.insert(TagField::Title, "Edited".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: read.format,
        tags: tags.clone(),
    })
    .expect("write tags");
    let atoms = mp4_freeform(&path);
    assert!(
        atoms.contains(&(
            "com.apple.iTunes".to_string(),
            "MYMULTI".to_string(),
            vec!["one".to_string(), "two".to_string()]
        )),
        "an unrelated edit collapsed the values, got {atoms:?}"
    );
    assert!(
        atoms.contains(&(
            "org.example".to_string(),
            "ELSEWHERE".to_string(),
            vec!["kept".to_string()]
        )),
        "an unrelated edit moved the atom to another namespace, got {atoms:?}"
    );

    // Editing the fields themselves writes the new value, still in the
    // namespace the file had.
    tags.insert(multi_field, "three".to_string());
    tags.insert(elsewhere_field, "changed".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: read.format,
        tags,
    })
    .expect("write tags");
    let atoms = mp4_freeform(&path);
    std::fs::remove_file(&path).ok();
    assert!(
        atoms.contains(&(
            "com.apple.iTunes".to_string(),
            "MYMULTI".to_string(),
            vec!["three".to_string()]
        )),
        "the edited value did not land, got {atoms:?}"
    );
    assert!(
        atoms.contains(&(
            "org.example".to_string(),
            "ELSEWHERE".to_string(),
            vec!["changed".to_string()]
        )),
        "an edit moved the atom to another namespace, got {atoms:?}"
    );
}

/// #609, the Vorbis side: a comment repeated under one name read as its last
/// value, and the next edit of anything wrote that value back alone.
#[test]
fn a_repeated_custom_comment_survives_an_unrelated_edit() {
    use lofty::config::WriteOptions;
    use lofty::ogg::tag::VorbisComments;
    use lofty::prelude::TagExt;

    let path = temp_flac_path("repeated-comment");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");
    let mut seeded = VorbisComments::default();
    seeded.push("ARTIST".to_string(), "Original".to_string());
    seeded.push("SOMEONE_ELSES_FIELD".to_string(), "one".to_string());
    seeded.push("SOMEONE_ELSES_FIELD".to_string(), "two".to_string());
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();

    let read = TagEngine::read(&path).expect("read tags");
    assert_eq!(
        read.tags
            .get(&TagField::Custom("SOMEONE_ELSES_FIELD".to_string()))
            .map(String::as_str),
        Some("one; two")
    );
    let mut tags = read.tags.clone();
    tags.insert(TagField::Artist, "Edited".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags,
    })
    .expect("write tags");

    let values: Vec<String> = {
        use lofty::config::ParseOptions;
        use lofty::file::AudioFile;
        use lofty::flac::FlacFile;
        let mut file = std::fs::File::open(&path).unwrap();
        FlacFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .vorbis_comments()
            .map(|tag| {
                tag.get_all("SOMEONE_ELSES_FIELD")
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    std::fs::remove_file(&path).ok();
    assert_eq!(values, vec!["one".to_string(), "two".to_string()]);
}

/// #610: MP4 defines `tmpo` as an integer, which is what other taggers write.
/// The read did not see that atom at all, setting a BPM left it in place next to
/// a second, text `tmpo`, and clearing every field still left it in the file.
#[test]
fn an_mp4_integer_tempo_is_read_written_and_cleared() {
    use lofty::config::WriteOptions;
    use lofty::mp4::{Atom, AtomData, AtomIdent};
    use lofty::prelude::TagExt;

    let tempo_atoms = |path: &PathBuf| -> Vec<Vec<String>> {
        mp4_ilst(path)
            .into_iter()
            .filter(|atom| atom.ident() == &AtomIdent::Fourcc(*b"tmpo"))
            .map(|atom| atom.data().map(|data| format!("{data:?}")).collect())
            .collect()
    };

    let path = temp_m4a_path("tempo");
    std::fs::write(&path, MINIMAL_M4A).expect("write fixture");
    let mut ilst = mp4_ilst(&path);
    ilst.insert(Atom::new(
        AtomIdent::Fourcc(*b"tmpo"),
        AtomData::SignedInteger(128),
    ));
    ilst.save_to_path(&path, WriteOptions::default()).unwrap();

    let read = TagEngine::read(&path).expect("read tags");
    assert_eq!(
        read.tags.get(&TagField::Bpm).map(String::as_str),
        Some("128")
    );

    let mut tags = read.tags.clone();
    tags.insert(TagField::Bpm, "124".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: read.format,
        tags,
    })
    .expect("write tags");
    assert_eq!(
        tempo_atoms(&path),
        vec![vec!["SignedInteger(124)".to_string()]],
        "the tempo must be one integer atom"
    );
    // Two bytes, the size the format defines and some readers insist on: the
    // atom is its 8-byte header, a 16-byte `data` header (size, name, type 21
    // for a big-endian signed integer, locale) and the value.
    let bytes = std::fs::read(&path).unwrap();
    let at = bytes
        .windows(4)
        .position(|window| window == b"tmpo")
        .unwrap();
    assert_eq!(&bytes[at - 4..at], &[0, 0, 0, 0x1a]);
    assert_eq!(&bytes[at + 12..at + 16], &[0, 0, 0, 0x15]);
    assert_eq!(&bytes[at + 20..at + 22], &124i16.to_be_bytes());
    assert_eq!(
        TagEngine::read(&path)
            .expect("read tags")
            .tags
            .get(&TagField::Bpm)
            .map(String::as_str),
        Some("124")
    );

    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: read.format,
        tags: BTreeMap::new(),
    })
    .expect("write tags");
    let left = tempo_atoms(&path);
    std::fs::remove_file(&path).ok();
    assert!(
        left.is_empty(),
        "clearing every field left the tempo, got {left:?}"
    );
}

/// #611: many taggers write a `tmpo` of 0 to mean there is no tempo. It read
/// as a BPM of 0, and the next edit wrote that 0 back as the model's value.
#[test]
fn an_mp4_tempo_of_zero_is_no_tempo_and_stays_as_it_was() {
    use lofty::config::WriteOptions;
    use lofty::mp4::{Atom, AtomData, AtomIdent};
    use lofty::prelude::TagExt;

    let path = temp_m4a_path("zero-tempo");
    std::fs::write(&path, MINIMAL_M4A).expect("write fixture");
    let mut ilst = mp4_ilst(&path);
    let mut zero = Atom::new(AtomIdent::Fourcc(*b"tmpo"), AtomData::SignedInteger(0));
    zero.push_data(AtomData::SignedInteger(0));
    ilst.insert(zero);
    ilst.save_to_path(&path, WriteOptions::default()).unwrap();

    let read = TagEngine::read(&path).expect("read tags");
    assert_eq!(read.tags.get(&TagField::Bpm), None);

    let mut tags = read.tags.clone();
    tags.insert(TagField::Title, "Edited".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: read.format,
        tags,
    })
    .expect("write tags");
    let tempo: Vec<Vec<String>> = mp4_ilst(&path)
        .into_iter()
        .filter(|atom| atom.ident() == &AtomIdent::Fourcc(*b"tmpo"))
        .map(|atom| atom.data().map(|data| format!("{data:?}")).collect())
        .collect();
    std::fs::remove_file(&path).ok();
    assert_eq!(
        tempo,
        vec![vec![
            "SignedInteger(0)".to_string(),
            "SignedInteger(0)".to_string()
        ]],
        "an unrelated edit rewrote the atom"
    );
}

/// #613: the URL went out under ID3v2's `WOAF` key, which only ID3v2 maps; on
/// every other tag type the backend dropped it on save, without an error.
#[test]
fn the_url_is_kept_on_every_tag_type() {
    let url = "https://example.com/release/12345".to_string();
    let fixtures: [(&str, &[u8]); 3] = [
        ("flac", &MINIMAL_FLAC),
        ("m4a", MINIMAL_M4A),
        ("wv", &MINIMAL_WAVPACK),
    ];
    for (extension, bytes) in fixtures {
        let path = std::env::temp_dir().join(format!(
            "tagrex-tag-engine-test-url-{}.{extension}",
            std::process::id()
        ));
        std::fs::write(&path, bytes).expect("write fixture");
        let read = TagEngine::read(&path).expect("read tags");
        let mut tags = read.tags.clone();
        tags.insert(TagField::Url, url.clone());
        TagEngine::write(&TrackFile {
            path: path.clone(),
            format: read.format,
            tags,
        })
        .expect("write tags");
        let back = TagEngine::read(&path).expect("read tags").tags;
        std::fs::remove_file(&path).ok();
        assert_eq!(back.get(&TagField::Url), Some(&url), "lost on .{extension}");
        assert_eq!(
            back.get(&TagField::Custom("WWW".to_string())),
            None,
            "the URL must read back as the URL on .{extension}"
        );
    }
}

/// #613: the `WWW` comment other taggers write is the URL.
#[test]
fn a_www_comment_reads_as_the_url() {
    use lofty::config::WriteOptions;
    use lofty::ogg::tag::VorbisComments;
    use lofty::prelude::TagExt;

    let path = temp_flac_path("www-comment");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");
    let mut seeded = VorbisComments::default();
    seeded.push("www".to_string(), "https://example.com/a".to_string());
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();
    let read = TagEngine::read(&path).expect("read tags");
    std::fs::remove_file(&path).ok();
    assert_eq!(
        read.tags.get(&TagField::Url).map(String::as_str),
        Some("https://example.com/a")
    );
}

/// #614: an APE tag that ends the file with `TAG` at 128 bytes from the end —
/// a 131-byte one always does, from its own header — is misread by the tag
/// library as an ID3v1 tag, and its writer then writes over the APE tag. Such
/// a file is refused before anything is touched.
#[test]
fn an_ape_tag_misread_as_id3v1_is_never_written_over() {
    use lofty::ape::{ApeItem, ApeTag};
    use lofty::config::WriteOptions;
    use lofty::prelude::TagExt;
    use lofty::tag::ItemValue;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-test-misread-id3v1-{}.wv",
        std::process::id()
    ));
    std::fs::write(&path, MINIMAL_WAVPACK).expect("write fixture");
    // Header and footer are 32 bytes each; one item of 8 + "WWW\0" + 55 bytes
    // makes the 131.
    let mut ape = ApeTag::new();
    ape.insert(ApeItem::new("WWW".to_string(), ItemValue::Text("x".repeat(55))).unwrap());
    ape.save_to_path(&path, WriteOptions::default()).unwrap();
    let before = std::fs::read(&path).unwrap();
    let tail = &before[before.len() - 128..];
    assert_eq!(&tail[..3], b"TAG", "fixture does not have the shape");
    assert_eq!(
        &tail[96..104],
        b"APETAGEX",
        "fixture does not have the shape"
    );

    let read = TagEngine::read(&path).expect("read tags");
    let mut tags = read.tags.clone();
    tags.insert(TagField::Title, "New title".to_string());
    let result = TagEngine::write(&TrackFile {
        path: path.clone(),
        format: read.format,
        tags,
    });
    let covers = TagEngine::write_covers(&path, &[]);
    let after = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(result.is_err(), "the write went ahead");
    assert!(covers.is_err(), "the cover write went ahead");
    assert_eq!(before, after, "the file was touched");
}

/// #615: an ID3v2 tag holding only a picture hid the ID3v1 tag beside it — the
/// file read as having no fields — and the next edit, keeping ID3v1 in step,
/// rewrote it with the nothing it had read.
#[test]
fn values_in_id3v1_survive_an_id3v2_tag_that_holds_only_a_picture() {
    use lofty::config::WriteOptions;
    use lofty::id3::v1::Id3v1Tag;
    use lofty::prelude::TagExt;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-test-v1-behind-picture-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");
    let mut v1 = Id3v1Tag::new();
    v1.title = Some("V1 Title".to_string());
    v1.artist = Some("V1 Artist".to_string());
    v1.save_to_path(&path, WriteOptions::default()).unwrap();

    let cover = CoverArt {
        mime: "image/png".to_string(),
        data: vec![0x89, 0x50, 0x4e, 0x47, 1, 2, 3, 4, 5],
        ..CoverArt::default()
    };
    TagEngine::write_covers(&path, std::slice::from_ref(&cover)).unwrap();

    let read = TagEngine::read_with_props(&path).expect("read tags");
    assert_eq!(
        read.file.tags.get(&TagField::Title).map(String::as_str),
        Some("V1 Title"),
        "the ID3v1 values are hidden"
    );
    assert!(
        read.blocks
            .iter()
            .any(|block| block.kind == TagBlockKind::Id3v1 && block.read_from),
        "the report must say the values come from ID3v1, got {:?}",
        read.blocks
    );

    let mut tags = read.file.tags.clone();
    tags.insert(TagField::Genre, "House".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Mp3,
        tags,
    })
    .expect("write tags");
    let after = TagEngine::read(&path).expect("read tags").tags;
    let v1_after = {
        use lofty::config::ParseOptions;
        use lofty::file::AudioFile;
        use lofty::mpeg::MpegFile;
        let mut file = std::fs::File::open(&path).unwrap();
        MpegFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .id3v1()
            .cloned()
    };
    std::fs::remove_file(&path).ok();
    assert_eq!(
        after.get(&TagField::Title).map(String::as_str),
        Some("V1 Title")
    );
    assert_eq!(
        after.get(&TagField::Artist).map(String::as_str),
        Some("V1 Artist")
    );
    let v1_after = v1_after.expect("ID3v1 tag kept");
    assert_eq!(v1_after.title.as_deref(), Some("V1 Title"), "ID3v1 erased");
    assert_eq!(
        v1_after.artist.as_deref(),
        Some("V1 Artist"),
        "ID3v1 erased"
    );
}

/// #615, the other half: most ID3v1 tags behind an empty ID3v2 tag are
/// zero-filled, and their zero genre byte decodes to "Blues". Such a tag names
/// no track, so it is not read in the empty block's place.
#[test]
fn a_zero_filled_id3v1_tag_is_not_read_in_place_of_an_empty_id3v2_tag() {
    use lofty::config::WriteOptions;
    use lofty::id3::v1::Id3v1Tag;
    use lofty::prelude::TagExt;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-test-zero-v1-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");
    let mut v1 = Id3v1Tag::new();
    v1.genre = Some(0);
    v1.comment = Some(" ".repeat(28));
    v1.save_to_path(&path, WriteOptions::default()).unwrap();
    TagEngine::write_covers(
        &path,
        &[CoverArt {
            mime: "image/png".to_string(),
            data: vec![0x89, 0x50, 0x4e, 0x47, 1, 2, 3, 4, 5],
            ..CoverArt::default()
        }],
    )
    .unwrap();

    let read = TagEngine::read_with_props(&path).expect("read tags");
    std::fs::remove_file(&path).ok();
    assert_eq!(read.file.tags, BTreeMap::new());
    assert!(
        read.blocks
            .iter()
            .any(|block| block.kind == TagBlockKind::Id3v2 && block.read_from),
        "got {:?}",
        read.blocks
    );
}

/// #618: setting a BPM on an M4A took the tempo atoms out with a swap, which
/// moved the first value of a multi-value field out of its place.
#[test]
fn an_m4a_save_with_a_bpm_keeps_multi_values_in_order() {
    let path = temp_m4a_path("order-with-bpm");
    std::fs::write(&path, MINIMAL_M4A).expect("write fixture");
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Artist One; Artist Two".to_string());
    tags.insert(TagField::Bpm, "124".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::M4a,
        tags,
    })
    .expect("write tags");
    let back = TagEngine::read(&path).expect("read tags").tags;
    std::fs::remove_file(&path).ok();
    assert_eq!(
        back.get(&TagField::Artist).map(String::as_str),
        Some("Artist One; Artist Two")
    );
}

/// #616: a block's content was only what the generic tag holds, so a
/// conversion wrote the new block without the custom fields and removed the
/// old one, while the preview called them kept.
#[test]
fn a_conversion_carries_custom_fields_or_names_them_as_lost() {
    let custom = TagField::Custom("TAGREX_CUSTOM_TEST".to_string());
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Title, "Title".to_string());
    tags.insert(TagField::Artist, "Artist One; Artist Two".to_string());
    tags.insert(custom.clone(), "custom value".to_string());

    // Into APE, which holds items by name: carried, every value kept.
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-test-convert-ape-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Mp3,
        tags: tags.clone(),
    })
    .expect("write tags");
    let content = TagEngine::read_block(&path, TagBlockKind::Id3v2)
        .unwrap()
        .expect("an ID3v2 block");
    assert_eq!(
        content.tags.get(&custom).map(String::as_str),
        Some("custom value"),
        "the block's content leaves the custom field out"
    );
    assert_eq!(content.conversion_loss(TagBlockKind::Ape).fields, vec![]);
    TagEngine::write_block(&path, TagBlockKind::Ape, None, &content).unwrap();
    TagEngine::remove_block(&path, TagBlockKind::Id3v2).unwrap();
    let ape = TagEngine::read_block(&path, TagBlockKind::Ape)
        .unwrap()
        .expect("an APE block");
    std::fs::remove_file(&path).ok();
    assert_eq!(
        ape.tags.get(&custom).map(String::as_str),
        Some("custom value")
    );
    assert_eq!(
        ape.tags.get(&TagField::Artist).map(String::as_str),
        Some("Artist One; Artist Two")
    );

    // Into RIFF INFO, which has no names to hold it under: named as lost.
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-test-convert-riff-{}.wav",
        std::process::id()
    ));
    std::fs::write(&path, minimal_wav()).expect("write fixture");
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Wav,
        tags,
    })
    .expect("write tags");
    let content = TagEngine::read_block(&path, TagBlockKind::Id3v2)
        .unwrap()
        .expect("an ID3v2 block");
    std::fs::remove_file(&path).ok();
    assert!(
        content
            .conversion_loss(TagBlockKind::RiffInfo)
            .fields
            .contains(&custom),
        "the preview does not name the custom field"
    );
}

/// #616: putting a Vorbis block back (undo of its removal) keeps its custom
/// fields and the order of a multi-value field.
#[test]
fn a_vorbis_block_written_back_keeps_custom_fields_and_order() {
    let path = temp_flac_path("vorbis-block-back");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");
    let custom = TagField::Custom("TAGREX_CUSTOM_TEST".to_string());
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Artist One; Artist Two".to_string());
    tags.insert(custom.clone(), "custom value".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags,
    })
    .expect("write tags");
    let content = TagEngine::read_block(&path, TagBlockKind::VorbisComments)
        .unwrap()
        .expect("a Vorbis block");
    TagEngine::remove_block(&path, TagBlockKind::VorbisComments).unwrap();
    TagEngine::write_block(&path, TagBlockKind::VorbisComments, None, &content).unwrap();
    let back = TagEngine::read(&path).expect("read tags").tags;
    std::fs::remove_file(&path).ok();
    assert_eq!(
        back.get(&TagField::Artist).map(String::as_str),
        Some("Artist One; Artist Two")
    );
    assert_eq!(back.get(&custom).map(String::as_str), Some("custom value"));
}

/// #617: ID3v1 holds 30 bytes of Latin-1 per field. The preview measured the
/// tag in memory, where it holds anything, and so called a Cyrillic or an
/// over-long title kept.
#[test]
fn the_id3v1_preview_names_what_the_bytes_cannot_hold() {
    let content = TagBlockContent {
        tags: BTreeMap::from([
            (TagField::Title, "Заголовок".to_string()),
            (
                TagField::Album,
                "An album title well over thirty characters".to_string(),
            ),
            (TagField::Artist, "Plain Artist".to_string()),
        ]),
        covers: Vec::new(),
    };
    let lost = content.conversion_loss(TagBlockKind::Id3v1).fields;
    assert!(lost.contains(&TagField::Title), "got {lost:?}");
    assert!(lost.contains(&TagField::Album), "got {lost:?}");
    assert!(!lost.contains(&TagField::Artist), "got {lost:?}");
}

/// #619: the carry loop put the file's `POPM` frame back over the one written
/// from the model, so editing or clearing the rating never reached the file.
#[test]
fn an_edited_or_cleared_rating_reaches_the_file() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, Id3v2Tag, PopularimeterFrame};
    use lofty::mpeg::MpegFile;
    use lofty::prelude::TagExt;

    let ratings = |path: &PathBuf| -> Vec<(String, u8, u64)> {
        let mut file = std::fs::File::open(path).unwrap();
        let mut found: Vec<(String, u8, u64)> = MpegFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .id3v2()
            .map(|tag| {
                tag.into_iter()
                    .filter_map(|frame| match frame {
                        Frame::Popularimeter(p) => Some((p.email.to_string(), p.rating, p.counter)),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        found
    };
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-test-rating-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");
    let mut seeded = Id3v2Tag::new();
    seeded.insert(Frame::Popularimeter(PopularimeterFrame::new(
        "other@y", 64, 1,
    )));
    // 150 is no star value: it reads as 4 stars, which write as 196.
    seeded.insert(Frame::Popularimeter(PopularimeterFrame::new(
        "rater@x", 150, 3,
    )));
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();
    let field = TagField::Custom("POPM".to_string());
    let write = |tags: BTreeMap<TagField, String>| {
        TagEngine::write(&TrackFile {
            path: path.clone(),
            format: AudioFormat::Mp3,
            tags,
        })
        .expect("write tags")
    };

    let read = TagEngine::read(&path).expect("read tags");
    assert_eq!(
        read.tags.get(&field).map(String::as_str),
        Some("rater@x|4|3")
    );

    // An unrelated edit keeps both frames exactly.
    let mut tags = read.tags.clone();
    tags.insert(TagField::Title, "Edited".to_string());
    write(tags.clone());
    assert_eq!(
        ratings(&path),
        vec![
            ("other@y".to_string(), 64, 1),
            ("rater@x".to_string(), 150, 3)
        ]
    );

    // An edit lands; the other rater's frame stays.
    tags.insert(field.clone(), "rater@x|5|3".to_string());
    write(tags.clone());
    assert_eq!(
        ratings(&path),
        vec![
            ("other@y".to_string(), 64, 1),
            ("rater@x".to_string(), 255, 3)
        ]
    );
    let reread = TagEngine::read(&path).expect("read tags");
    assert_eq!(
        reread.tags.get(&field).map(String::as_str),
        Some("rater@x|5|3")
    );

    // So does a clear.
    tags.remove(&field);
    write(tags);
    let left = ratings(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(left, vec![("other@y".to_string(), 64, 1)]);
}

/// A minimal AIFF (8 kHz mono 8-bit silence) lofty accepts.
fn minimal_aiff() -> Vec<u8> {
    let frames: u32 = 800;
    let mut comm = Vec::new();
    comm.extend_from_slice(&1u16.to_be_bytes()); // mono
    comm.extend_from_slice(&frames.to_be_bytes());
    comm.extend_from_slice(&8u16.to_be_bytes());
    // 8000 Hz as an 80-bit extended float.
    comm.extend_from_slice(&[0x40, 0x0B, 0xFA, 0, 0, 0, 0, 0, 0, 0]);
    let mut ssnd = vec![0u8; 8]; // offset, block size
    ssnd.resize(8 + frames as usize, 0);
    let mut body = Vec::new();
    for (id, payload) in [(b"COMM", &comm), (b"SSND", &ssnd)] {
        body.extend_from_slice(id);
        body.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        body.extend_from_slice(payload);
    }
    let mut data = Vec::new();
    data.extend_from_slice(b"FORM");
    data.extend_from_slice(&((4 + body.len()) as u32).to_be_bytes());
    data.extend_from_slice(b"AIFF");
    data.extend_from_slice(&body);
    data
}

/// #621: a WAV or an AIFF is written as ID3v2, so a RIFF INFO list or AIFF
/// text chunks the file arrived with kept stating the old values beside it.
/// They follow the model now, keep what the model has no field for, and are
/// never created.
#[test]
fn a_wav_or_aiff_keeps_its_legacy_text_block_in_step() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::iff::aiff::{AiffFile, AiffTextChunks};
    use lofty::iff::wav::{RiffInfoList, WavFile};
    use lofty::prelude::TagExt;

    let write = |path: &PathBuf, format: AudioFormat, tags: &BTreeMap<TagField, String>| {
        TagEngine::write(&TrackFile {
            path: path.clone(),
            format,
            tags: tags.clone(),
        })
        .expect("write tags")
    };

    // WAV.
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-riff-info-{}.wav",
        std::process::id()
    ));
    std::fs::write(&path, minimal_wav()).expect("write fixture");
    let mut info = RiffInfoList::new();
    for (id, value) in [
        ("INAM", "Old title"),
        ("IART", "Artist"),
        ("ICRD", "2001-05-17"),
        ("ITRK", "3/12"),
        ("ISFT", "Some encoder"),
        ("ICOP", "(c) Label"),
    ] {
        info.insert(id.to_string(), value.to_string());
    }
    info.save_to_path(&path, WriteOptions::default()).unwrap();
    let riff = |path: &PathBuf| -> Vec<(String, String)> {
        let mut file = std::fs::File::open(path).unwrap();
        WavFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .riff_info()
            .map(|list| {
                let mut items: Vec<(String, String)> = list
                    .into_iter()
                    .map(|(id, value)| (id.clone(), value.trim_end_matches('\0').to_string()))
                    .collect();
                // The backend's writer does not keep the list's order.
                items.sort();
                items
            })
            .unwrap_or_default()
    };
    let mut tags = TagEngine::read(&path).expect("read tags").tags;
    tags.insert(TagField::Title, "New title".to_string());
    tags.insert(TagField::Genre, "House".to_string());
    // An ID3v2 year beside a full INFO date.
    tags.insert(TagField::Year, "2001".to_string());
    write(&path, AudioFormat::Wav, &tags);
    let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
        let mut items: Vec<(String, String)> = list
            .iter()
            .map(|(id, value)| (id.to_string(), value.to_string()))
            .collect();
        items.sort();
        items
    };
    assert_eq!(
        riff(&path),
        pairs(&[
            ("INAM", "New title"),
            ("IART", "Artist"),
            // A full date agreeing with the year is kept whole.
            ("ICRD", "2001-05-17"),
            // The file's own id and notation for the track number.
            ("ITRK", "3/12"),
            ("ISFT", "Some encoder"),
            ("ICOP", "(c) Label"),
            // No `IGNR`: a field the block never stated is not added.
        ])
    );
    assert_eq!(
        TagEngine::read(&path)
            .unwrap()
            .tags
            .get(&TagField::Title)
            .map(String::as_str),
        Some("New title")
    );

    // A clear takes out what the model owns; the rest stays.
    write(&path, AudioFormat::Wav, &BTreeMap::new());
    assert_eq!(
        riff(&path),
        pairs(&[("ISFT", "Some encoder"), ("ICOP", "(c) Label")])
    );

    // A file without one does not get one.
    std::fs::write(&path, minimal_wav()).expect("write fixture");
    write(&path, AudioFormat::Wav, &tags);
    assert!(riff(&path).is_empty());
    std::fs::remove_file(&path).ok();

    // AIFF.
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-aiff-text-{}.aiff",
        std::process::id()
    ));
    std::fs::write(&path, minimal_aiff()).expect("write fixture");
    let text = AiffTextChunks {
        name: Some("Old title".to_string()),
        author: Some("Artist".to_string()),
        copyright: Some("(c) Label".to_string()),
        annotations: Some(vec!["Old note".to_string()]),
        comments: None,
    };
    text.save_to_path(&path, WriteOptions::default()).unwrap();
    let chunks = |path: &PathBuf| {
        let mut file = std::fs::File::open(path).unwrap();
        AiffFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .text_chunks()
            .cloned()
    };
    let mut tags = TagEngine::read(&path).expect("read tags").tags;
    tags.insert(TagField::Title, "New title".to_string());
    tags.insert(TagField::Comment, "New note".to_string());
    write(&path, AudioFormat::Aiff, &tags);
    let after = chunks(&path).expect("text chunks kept");
    assert_eq!(after.name.as_deref(), Some("New title"));
    assert_eq!(after.author.as_deref(), Some("Artist"));
    assert_eq!(after.copyright.as_deref(), Some("(c) Label"));
    assert_eq!(after.annotations, Some(vec!["New note".to_string()]));

    tags.remove(&TagField::Title);
    tags.remove(&TagField::Artist);
    tags.remove(&TagField::Comment);
    write(&path, AudioFormat::Aiff, &tags);
    let after = chunks(&path).expect("copyright kept");
    assert_eq!(after.name, None);
    assert_eq!(after.author, None);
    assert_eq!(after.annotations, None);
    assert_eq!(after.copyright.as_deref(), Some("(c) Label"));
    std::fs::remove_file(&path).ok();
}

/// #652: a file that also has an ID3v2 chunk is read from that chunk alone, so
/// a field only its INFO list or AIFF text chunks state never reaches the model,
/// and the sync of #621 took that for a clear and deleted it on any save.
#[test]
fn a_legacy_text_item_the_id3v2_chunk_lacks_survives_a_save() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::Id3v2Tag;
    use lofty::iff::aiff::{AiffFile, AiffTextChunks};
    use lofty::iff::wav::{RiffInfoList, WavFile};
    use lofty::prelude::{Accessor, TagExt};

    let title_only = || {
        let mut id3 = Id3v2Tag::new();
        id3.set_title("Only title".to_string());
        id3
    };

    // WAV.
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-info-beside-id3-{}.wav",
        std::process::id()
    ));
    std::fs::write(&path, minimal_wav()).expect("write fixture");
    let mut info = RiffInfoList::new();
    for (id, value) in [("INAM", "Old title"), ("IART", "Artist"), ("IGNR", "House")] {
        info.insert(id.to_string(), value.to_string());
    }
    info.save_to_path(&path, WriteOptions::default()).unwrap();
    title_only()
        .save_to_path(&path, WriteOptions::default())
        .unwrap();
    let mut file = TagEngine::read(&path).expect("read");
    assert_eq!(
        file.tags.get(&TagField::Genre),
        None,
        "read from ID3v2 alone"
    );
    file.tags.insert(TagField::Title, "New title".to_string());
    TagEngine::write(&file).expect("write");
    let mut handle = std::fs::File::open(&path).unwrap();
    let mut items: Vec<(String, String)> = WavFile::read_from(&mut handle, ParseOptions::new())
        .unwrap()
        .riff_info()
        .expect("INFO kept")
        .into_iter()
        .map(|(id, value)| (id.clone(), value.trim_end_matches('\0').to_string()))
        .collect();
    items.sort();
    std::fs::remove_file(&path).ok();
    let expected = [("IART", "Artist"), ("IGNR", "House"), ("INAM", "New title")];
    assert_eq!(
        items,
        expected.map(|(id, value)| (id.to_string(), value.to_string()))
    );

    // AIFF.
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-text-beside-id3-{}.aiff",
        std::process::id()
    ));
    std::fs::write(&path, minimal_aiff()).expect("write fixture");
    let text = AiffTextChunks {
        name: Some("Old title".to_string()),
        author: Some("Artist".to_string()),
        copyright: None,
        annotations: Some(vec!["A note".to_string()]),
        comments: None,
    };
    text.save_to_path(&path, WriteOptions::default()).unwrap();
    title_only()
        .save_to_path(&path, WriteOptions::default())
        .unwrap();
    let file = TagEngine::read(&path).expect("read");
    TagEngine::write(&file).expect("write");
    let mut handle = std::fs::File::open(&path).unwrap();
    let after = AiffFile::read_from(&mut handle, ParseOptions::new())
        .unwrap()
        .text_chunks()
        .cloned()
        .expect("text chunks kept");
    std::fs::remove_file(&path).ok();
    assert_eq!(after.name.as_deref(), Some("Only title"));
    assert_eq!(after.author.as_deref(), Some("Artist"));
    assert_eq!(after.annotations, Some(vec!["A note".to_string()]));
}

/// #623: the backend builds an ID3v2 tag's frames from a hash set, so the same
/// tags came out in a different order on every save. Saves of the same tags
/// are byte for byte the same now, and the frames a file already has keep the
/// file's own order.
#[test]
fn saving_the_same_tags_twice_writes_the_same_bytes() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, FrameId, Id3v2Tag, TextInformationFrame};
    use lofty::iff::wav::WavFile;
    use lofty::mpeg::MpegFile;
    use lofty::prelude::TagExt;
    use lofty::TextEncoding;
    use std::borrow::Cow;

    // The order a file might arrive in, nothing like the backend's own.
    let seeded_ids = [
        "TCON", "TPE1", "TRCK", "TALB", "TIT2", "TPE2", "TCOM", "TPUB", "TBPM", "TKEY",
    ];
    for (format, bytes, extension) in [
        (AudioFormat::Mp3, minimal_mp3(), "mp3"),
        (AudioFormat::Wav, minimal_wav(), "wav"),
    ] {
        let path = std::env::temp_dir().join(format!(
            "tagrex-tag-engine-frame-order-{}.{extension}",
            std::process::id()
        ));
        std::fs::write(&path, bytes).expect("write fixture");
        let mut seeded = Id3v2Tag::new();
        for id in seeded_ids {
            let value = if id == "TRCK" { "3" } else { id };
            seeded.insert(Frame::Text(TextInformationFrame::new(
                FrameId::Valid(Cow::Borrowed(id)),
                TextEncoding::UTF8,
                value.to_string(),
            )));
        }
        seeded.save_to_path(&path, WriteOptions::default()).unwrap();

        let mut tags = TagEngine::read(&path).expect("read tags").tags;
        tags.insert(TagField::Title, "Edited".to_string());
        tags.insert(TagField::Year, "2001".to_string());
        tags.insert(TagField::Comment, "New".to_string());
        let write = || {
            TagEngine::write(&TrackFile {
                path: path.clone(),
                format,
                tags: tags.clone(),
            })
            .expect("write tags");
            std::fs::read(&path).unwrap()
        };
        let first = write();
        for _ in 0..4 {
            assert!(
                write() == first,
                "{extension}: a second save changed the bytes"
            );
        }

        let mut file = std::fs::File::open(&path).unwrap();
        let tag = if format == AudioFormat::Mp3 {
            MpegFile::read_from(&mut file, ParseOptions::new())
                .unwrap()
                .id3v2()
                .cloned()
        } else {
            WavFile::read_from(&mut file, ParseOptions::new())
                .unwrap()
                .id3v2()
                .cloned()
        };
        let ids: Vec<String> = tag
            .unwrap()
            .into_iter()
            .map(|frame| frame.id_str().to_string())
            .collect();
        std::fs::remove_file(&path).ok();
        // The file's frames first, in the file's order, then the new ones.
        assert_eq!(ids[..seeded_ids.len()], seeded_ids, "{extension}");
        assert_eq!(ids[seeded_ids.len()..], ["COMM", "TDRC"], "{extension}");
    }
}

/// #638: a save dropped the leading zero from a track or disc number. ID3v2's
/// `TRCK`/`TPOS` and APE's `Track`/`Disc` were built from the numbers as
/// integers, and a Vorbis read parsed `TRACKNUMBER` into one, so `01/03` came
/// back as `1/3` after an edit of something else. MP4 keeps the numbers as
/// integers, so it has no zero to keep and is not checked.
#[test]
fn a_track_number_keeps_its_leading_zero() {
    let fixtures: Vec<(&str, Vec<u8>)> = vec![
        ("flac", MINIMAL_FLAC.to_vec()),
        ("mp3", minimal_mp3()),
        ("wav", minimal_wav()),
        ("aiff", minimal_aiff()),
        ("wv", MINIMAL_WAVPACK.to_vec()),
    ];
    let numbers = [
        (TagField::TrackNumber, "01"),
        (TagField::TrackTotal, "03"),
        (TagField::DiscNumber, "1"),
        (TagField::DiscTotal, "02"),
    ];
    for (extension, bytes) in fixtures {
        let path = std::env::temp_dir().join(format!(
            "tagrex-tag-engine-test-leading-zero-{}.{extension}",
            std::process::id()
        ));
        std::fs::write(&path, bytes).expect("write fixture");
        let read = TagEngine::read(&path).expect("read tags");
        let mut tags = read.tags.clone();
        tags.insert(TagField::Title, "Title".to_string());
        for (field, value) in &numbers {
            tags.insert(field.clone(), value.to_string());
        }
        TagEngine::write(&TrackFile {
            path: path.clone(),
            format: read.format,
            tags,
        })
        .expect("write tags");
        let back = TagEngine::read(&path).expect("read tags");
        for (field, value) in &numbers {
            assert_eq!(
                back.tags.get(field).map(String::as_str),
                Some(*value),
                "{field:?} on .{extension}"
            );
        }

        // An edit writes what was typed, zero or not.
        let mut tags = back.tags.clone();
        tags.insert(TagField::TrackNumber, "2".to_string());
        TagEngine::write(&TrackFile {
            path: path.clone(),
            format: back.format,
            tags,
        })
        .expect("write tags");
        let edited = TagEngine::read(&path).expect("read tags").tags;
        std::fs::remove_file(&path).ok();
        assert_eq!(
            edited.get(&TagField::TrackNumber).map(String::as_str),
            Some("2"),
            "edited number on .{extension}"
        );
        assert_eq!(
            edited.get(&TagField::TrackTotal).map(String::as_str),
            Some("03"),
            "untouched total on .{extension}"
        );
    }

    // A Vorbis comment stating the pair in one, as another tagger writes it.
    use lofty::config::WriteOptions;
    use lofty::ogg::tag::VorbisComments;
    use lofty::prelude::TagExt;
    let path = temp_flac_path("leading-zero-pair");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");
    let mut comments = VorbisComments::new();
    comments.insert("TRACKNUMBER".to_string(), "07/12".to_string());
    comments.insert("DISCNUMBER".to_string(), "1/02".to_string());
    comments
        .save_to_path(&path, WriteOptions::default())
        .unwrap();
    let read = TagEngine::read(&path).expect("read tags").tags;
    std::fs::remove_file(&path).ok();
    for (field, value) in [
        (TagField::TrackNumber, "07"),
        (TagField::TrackTotal, "12"),
        (TagField::DiscNumber, "1"),
        (TagField::DiscTotal, "02"),
    ] {
        assert_eq!(
            read.get(&field).map(String::as_str),
            Some(value),
            "{field:?}"
        );
    }
}

/// #639: writing a block of its own, as a conversion or a revision change
/// does, built ID3v2's `TRCK` and APE's `Track` from the numbers as integers,
/// the way an ordinary save did before #638.
#[test]
fn a_block_write_keeps_the_leading_zero() {
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-test-block-zero-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Title, "Title".to_string());
    tags.insert(TagField::TrackNumber, "01".to_string());
    tags.insert(TagField::TrackTotal, "03".to_string());
    tags.insert(TagField::DiscNumber, "1".to_string());
    tags.insert(TagField::DiscTotal, "02".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Mp3,
        tags: tags.clone(),
    })
    .expect("write tags");
    let content = TagEngine::read_block(&path, TagBlockKind::Id3v2)
        .unwrap()
        .expect("an ID3v2 block");

    // The same block at another revision, then the same content as APE.
    TagEngine::write_block(
        &path,
        TagBlockKind::Id3v2,
        Some(Id3v2Revision::V3),
        &content,
    )
    .unwrap();
    let id3 = TagEngine::read_block(&path, TagBlockKind::Id3v2)
        .unwrap()
        .expect("an ID3v2 block");
    TagEngine::write_block(&path, TagBlockKind::Ape, None, &content).unwrap();
    let ape = TagEngine::read_block(&path, TagBlockKind::Ape)
        .unwrap()
        .expect("an APE block");
    std::fs::remove_file(&path).ok();
    for (kind, block) in [("ID3v2.3", &id3), ("APE", &ape)] {
        for field in [
            TagField::TrackNumber,
            TagField::TrackTotal,
            TagField::DiscNumber,
            TagField::DiscTotal,
        ] {
            assert_eq!(
                block.tags.get(&field),
                tags.get(&field),
                "{field:?} in {kind}"
            );
        }
    }
}

/// #640: a vinyl position in `TRCK`, as `B1/04`, is not a pair of numbers, so
/// the backend handed the whole text over as the track total and dropped it on
/// the way out: an MP3 showed a total of `B1/04` and lost the frame to any save.
/// APE read it right and wrote `0/04`, losing a disc side `A` with it.
#[test]
fn a_vinyl_track_position_survives_a_save() {
    use lofty::ape::{ApeItem, ApeTag};
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, FrameId, Id3v2Tag, TextInformationFrame};
    use lofty::mpeg::MpegFile;
    use lofty::prelude::TagExt;
    use lofty::tag::ItemValue;
    use lofty::TextEncoding;

    let pairs = |tags: &BTreeMap<TagField, String>| {
        [
            TagField::TrackNumber,
            TagField::TrackTotal,
            TagField::DiscNumber,
            TagField::DiscTotal,
        ]
        .map(|field| tags.get(&field).cloned())
    };
    let some = |value: &str| Some(value.to_string());
    let frame_text = |path: &PathBuf, id: &str| -> Option<String> {
        let mut file = std::fs::File::open(path).unwrap();
        MpegFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .id3v2()?
            .get_text(&FrameId::Valid(id.into()))
            .map(str::to_string)
    };

    for (trck, tpos, expected) in [
        ("B1/04", "A", [some("B1"), some("04"), some("A"), None]),
        ("A/04", "1/2", [some("A"), some("04"), some("1"), some("2")]),
        ("12a", "B/2", [some("12a"), None, some("B"), some("2")]),
    ] {
        let path = std::env::temp_dir().join(format!(
            "tagrex-tag-engine-test-vinyl-{}.mp3",
            std::process::id()
        ));
        std::fs::write(&path, minimal_mp3()).expect("write fixture");
        let mut seeded = Id3v2Tag::new();
        for (id, value) in [("TIT2", "Title"), ("TRCK", trck), ("TPOS", tpos)] {
            seeded.insert(Frame::Text(TextInformationFrame::new(
                FrameId::Valid(id.into()),
                TextEncoding::UTF8,
                value.to_string(),
            )));
        }
        seeded.save_to_path(&path, WriteOptions::default()).unwrap();
        let read = TagEngine::read(&path).expect("read tags");
        assert_eq!(pairs(&read.tags), expected, "read of {trck} {tpos}");

        // An unrelated edit puts both frames back as they were.
        let mut tags = read.tags.clone();
        tags.insert(TagField::Title, "Edited".to_string());
        TagEngine::write(&TrackFile {
            path: path.clone(),
            format: read.format,
            tags,
        })
        .expect("write tags");
        assert_eq!(frame_text(&path, "TRCK").as_deref(), Some(trck));
        assert_eq!(frame_text(&path, "TPOS").as_deref(), Some(tpos));

        // And so does a block write, as an ID3v2.3 or as APE.
        let content = TagEngine::read_block(&path, TagBlockKind::Id3v2)
            .unwrap()
            .expect("an ID3v2 block");
        assert_eq!(pairs(&content.tags), expected, "block of {trck} {tpos}");
        TagEngine::write_block(
            &path,
            TagBlockKind::Id3v2,
            Some(Id3v2Revision::V3),
            &content,
        )
        .unwrap();
        assert_eq!(frame_text(&path, "TRCK").as_deref(), Some(trck));
        TagEngine::write_block(&path, TagBlockKind::Ape, None, &content).unwrap();
        let ape = TagEngine::read_block(&path, TagBlockKind::Ape)
            .unwrap()
            .expect("an APE block");
        std::fs::remove_file(&path).ok();
        assert_eq!(pairs(&ape.tags), expected, "APE block of {trck} {tpos}");
    }

    // An APE tag of its own.
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-test-vinyl-{}.wv",
        std::process::id()
    ));
    std::fs::write(&path, MINIMAL_WAVPACK).expect("write fixture");
    let mut ape = ApeTag::new();
    for (key, value) in [("Title", "Title"), ("Track", "B1/04"), ("Disc", "A")] {
        ape.insert(ApeItem::new(key.to_string(), ItemValue::Text(value.to_string())).unwrap());
    }
    ape.save_to_path(&path, WriteOptions::default()).unwrap();
    let read = TagEngine::read(&path).expect("read tags");
    TagEngine::write(&read).expect("write tags");
    let back = TagEngine::read(&path).expect("read tags");
    std::fs::remove_file(&path).ok();
    assert_eq!(
        pairs(&back.tags),
        [some("B1"), some("04"), some("A"), None],
        "APE"
    );
}

/// What the file's primary tag holds for `key`, value by value, as the backend
/// reads it.
fn generic_values(path: &std::path::Path, key: lofty::tag::ItemKey) -> Vec<String> {
    use lofty::file::TaggedFileExt;
    let tagged = lofty::probe::Probe::open(path).unwrap().read().unwrap();
    tagged
        .primary_tag()
        .map(|tag| tag.get_strings(key).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Seed several `ARTISTS` and label values through the backend's generic tag,
/// which writes each format's own way of holding them.
fn seed_several_values(path: &std::path::Path) {
    use lofty::config::WriteOptions;
    use lofty::file::TaggedFileExt;
    use lofty::prelude::TagExt;
    use lofty::tag::{ItemKey, ItemValue, Tag, TagItem};
    let tagged = lofty::probe::Probe::open(path).unwrap().read().unwrap();
    let mut tag = tagged
        .primary_tag()
        .cloned()
        .unwrap_or_else(|| Tag::new(tagged.primary_tag_type()));
    for (key, value) in [
        (ItemKey::TrackArtists, "A"),
        (ItemKey::TrackArtists, "B"),
        (ItemKey::Label, "X"),
        (ItemKey::Label, "Y"),
    ] {
        tag.push(TagItem::new(key, ItemValue::Text(value.to_string())));
    }
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

/// #649: a field the app reads as one value but the file holds several of was
/// cut to the last one by an edit to any other field. Untouched, it keeps them
/// all; edited, it is the one value it was edited to.
#[test]
fn an_unrelated_edit_keeps_every_value_of_a_single_value_field() {
    use lofty::tag::ItemKey;
    for (path, fixture) in [
        (temp_flac_path("several-values"), MINIMAL_FLAC.as_slice()),
        (temp_m4a_path("several-values"), MINIMAL_M4A),
    ] {
        std::fs::write(&path, fixture).expect("write fixture");
        seed_several_values(&path);

        let read = retitle(&path, "Retitled");
        let artists = generic_values(&path, ItemKey::TrackArtists);
        let labels = generic_values(&path, ItemKey::Label);

        let mut edited = read.clone();
        edited.tags.insert(TagField::Publisher, "Z".to_string());
        TagEngine::write(&edited).expect("write the edit");
        let edited_labels = generic_values(&path, ItemKey::Label);
        std::fs::remove_file(&path).ok();

        let at = path.display();
        assert_eq!(artists, ["A", "B"], "{at}: ARTISTS lost values");
        assert_eq!(labels, ["X", "Y"], "{at}: the label lost values");
        assert_eq!(edited_labels, ["Z"], "{at}: the edited label");
    }
}

/// #649 for ID3v2, where several values share one text frame, null-separated.
#[test]
fn an_unrelated_edit_keeps_every_value_of_an_id3v2_text_frame() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{ExtendedTextFrame, Frame, FrameId, Id3v2Tag, TextInformationFrame};
    use lofty::mpeg::MpegFile;
    use lofty::prelude::TagExt;
    use lofty::TextEncoding;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-several-values-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).unwrap();
    let mut seeded = Id3v2Tag::new();
    seeded.insert(Frame::Text(TextInformationFrame::new(
        FrameId::new("TPUB").unwrap(),
        TextEncoding::UTF8,
        "X\0Y".to_string(),
    )));
    seeded.insert(Frame::UserText(ExtendedTextFrame::new(
        TextEncoding::UTF8,
        "ARTISTS".to_string(),
        "A\0B".to_string(),
    )));
    seeded.insert(Frame::UserText(ExtendedTextFrame::new(
        TextEncoding::UTF8,
        "MYMULTI".to_string(),
        "one\0two".to_string(),
    )));
    seeded.save_to_path(&path, WriteOptions::default()).unwrap();

    let frames = |path: &PathBuf| -> Vec<(String, String)> {
        let mut file = std::fs::File::open(path).unwrap();
        let mpeg = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
        let mut out: Vec<(String, String)> = mpeg
            .id3v2()
            .unwrap()
            .into_iter()
            .filter_map(|frame| match frame {
                Frame::Text(text) if frame.id().as_str() == "TPUB" => {
                    Some(("TPUB".to_string(), text.value.to_string()))
                }
                Frame::UserText(text) => Some((
                    format!("TXXX:{}", text.description),
                    text.content.to_string(),
                )),
                _ => None,
            })
            .collect();
        out.sort();
        out
    };

    let read = retitle(&path, "Retitled");
    let untouched = frames(&path);
    let mut edited = read.clone();
    edited.tags.insert(TagField::Publisher, "Z".to_string());
    TagEngine::write(&edited).expect("write the edit");
    let after_edit = frames(&path);
    std::fs::remove_file(&path).ok();

    let pair = |id: &str, value: &str| (id.to_string(), value.to_string());
    assert_eq!(
        untouched,
        [
            pair("TPUB", "X\0Y"),
            pair("TXXX:ARTISTS", "A\0B"),
            pair("TXXX:MYMULTI", "one\0two"),
        ]
    );
    assert!(after_edit.contains(&pair("TPUB", "Z")), "{after_edit:?}");
    assert!(
        after_edit.contains(&pair("TXXX:ARTISTS", "A\0B")),
        "{after_edit:?}"
    );
}

/// #650: a save wrote every field the backend knows under its one upper-case
/// name, so `title` became `TITLE` and a field stated under two names lost one.
/// A field nobody edited now goes back exactly as the file had it.
#[test]
fn an_unedited_vorbis_field_keeps_the_names_the_file_gave_it() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::flac::FlacFile;
    use lofty::ogg::tag::VorbisComments;
    use lofty::prelude::TagExt;

    let path = temp_flac_path("own-names");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");
    let seeded = [
        ("title", "Lower Title"),
        ("ORGANIZATION", "Org"),
        ("LABEL", "Lab"),
        ("TRACKTOTAL", "4"),
        ("totaltracks", "4"),
        ("Replaygain_Track_Gain", "-7.94 dB"),
    ];
    let mut comments = VorbisComments::new();
    for (name, value) in seeded {
        comments.push(name.to_string(), value.to_string());
    }
    comments
        .save_to_path(&path, WriteOptions::default())
        .unwrap();

    let sorted_comments = |path: &PathBuf| -> Vec<(String, String)> {
        let mut file = std::fs::File::open(path).unwrap();
        let flac = FlacFile::read_from(&mut file, ParseOptions::new()).unwrap();
        let mut items: Vec<(String, String)> = flac
            .vorbis_comments()
            .unwrap()
            .items()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        items.sort();
        items
    };

    let mut read = TagEngine::read(&path).expect("read");
    TagEngine::write(&read).expect("save unchanged");
    let unchanged = sorted_comments(&path);
    read.tags.insert(TagField::Publisher, "New".to_string());
    TagEngine::write(&read).expect("save the edit");
    let edited = sorted_comments(&path);
    std::fs::remove_file(&path).ok();

    let mut expected: Vec<(String, String)> = seeded
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    expected.sort();
    assert_eq!(unchanged, expected);
    // The edited field is one value, under a name the file used for it; the
    // others are left as they were.
    expected.retain(|(name, _)| name != "ORGANIZATION" && name != "LABEL");
    expected.push(("LABEL".to_string(), "New".to_string()));
    expected.sort();
    assert_eq!(edited, expected);
}

/// #654: a rating the backend cannot parse was dropped on write, so typing `5`
/// into the field deleted the file's rating. Such a value is refused now, with
/// the file untouched; the full form and a clear still work.
#[test]
fn a_rating_the_backend_cannot_parse_is_refused_not_deleted() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, Id3v2Tag, PopularimeterFrame};
    use lofty::mpeg::MpegFile;
    use lofty::prelude::{Accessor, TagExt};

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-bare-rating-{}.mp3",
        std::process::id()
    ));
    let ratings = || -> Vec<(String, u8, u64)> {
        let mut handle = std::fs::File::open(&path).unwrap();
        MpegFile::read_from(&mut handle, ParseOptions::new())
            .unwrap()
            .id3v2()
            .map(|tag| {
                tag.into_iter()
                    .filter_map(|frame| match frame {
                        Frame::Popularimeter(p) => Some((p.email.to_string(), p.rating, p.counter)),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let field = TagField::Custom("POPM".to_string());
    let seeded = || {
        std::fs::write(&path, minimal_mp3()).unwrap();
        let mut tag = Id3v2Tag::new();
        tag.set_title("Title".to_string());
        tag.insert(Frame::Popularimeter(PopularimeterFrame::new(
            "rater@x", 196, 3,
        )));
        tag.save_to_path(&path, WriteOptions::default()).unwrap();
        TagEngine::read(&path).expect("read")
    };

    for bad in ["5", "64", "rater@x|5", "rater@x|9|3"] {
        let mut file = seeded();
        file.tags.insert(field.clone(), bad.to_string());
        let error = TagEngine::write(&file).expect_err(bad).to_string();
        assert!(error.contains("invalid rating"), "{bad}: {error}");
        assert_eq!(ratings(), [("rater@x".to_string(), 196, 3)], "{bad}");
    }

    let mut file = seeded();
    file.tags
        .insert(field.clone(), "rater@example.com|5|0".to_string());
    TagEngine::write(&file).expect("the full form");
    assert_eq!(ratings(), [("rater@example.com".to_string(), 255, 0)]);

    let mut file = seeded();
    file.tags.insert(field.clone(), String::new());
    TagEngine::write(&file).expect("a clear");
    let cleared = ratings();
    std::fs::remove_file(&path).ok();
    assert!(cleared.is_empty(), "{cleared:?}");
}

/// #655: an MP3 whose only tag is an APE block was read from that block, but
/// written as ID3v2 alone. A clear left the APE block, so the file read back
/// unchanged; an edit put a new ID3v2 tag beside an APE block still stating the
/// old values.
#[test]
fn an_mp3_ape_block_follows_an_edit_and_a_clear() {
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-mp3-ape-sync-{}.mp3",
        std::process::id()
    ));
    let seeded = || {
        std::fs::write(&path, minimal_mp3()).unwrap();
        let mut content = TagBlockContent::default();
        content
            .tags
            .insert(TagField::Title, "Old Title".to_string());
        content.tags.insert(TagField::Artist, "Someone".to_string());
        content.tags.insert(TagField::Album, "Record".to_string());
        TagEngine::write_block(&path, TagBlockKind::Ape, None, &content).unwrap();
        TagEngine::read(&path).expect("read")
    };
    let ape = |path: &std::path::Path| {
        TagEngine::read_block(path, TagBlockKind::Ape)
            .unwrap()
            .map(|content| content.tags)
    };

    let file = seeded();
    assert_eq!(
        file.tags.get(&TagField::Title).map(String::as_str),
        Some("Old Title")
    );
    let edited = retitle(&path, "New Title");
    let block = ape(&path).expect("the APE block stays");
    assert_eq!(
        block.get(&TagField::Title).map(String::as_str),
        Some("New Title")
    );
    assert_eq!(
        block.get(&TagField::Artist).map(String::as_str),
        Some("Someone")
    );
    assert_eq!(
        edited.tags.get(&TagField::Title).map(String::as_str),
        Some("New Title")
    );

    let mut file = seeded();
    file.tags.clear();
    TagEngine::write(&file).expect("a clear");
    let block = ape(&path);
    let read_back = TagEngine::read(&path).expect("read back");
    std::fs::remove_file(&path).ok();
    assert_eq!(block, None);
    assert!(read_back.tags.is_empty(), "{:?}", read_back.tags);
}

/// #655, beside an ID3v2 tag: the model is read from ID3v2, so a field only the
/// APE block states and the block's picture survive an edit, and a save that
/// changed nothing leaves the block as it was.
#[test]
fn an_mp3_ape_block_beside_id3v2_loses_only_what_was_edited() {
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-mp3-ape-id3v2-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).unwrap();
    let mut content = TagBlockContent::default();
    content
        .tags
        .insert(TagField::Title, "Old Title".to_string());
    content
        .tags
        .insert(TagField::Album, "Only In APE".to_string());
    content.covers.push(CoverArt {
        mime: "image/png".to_string(),
        data: vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 7],
        ..CoverArt::default()
    });
    TagEngine::write_block(&path, TagBlockKind::Ape, None, &content).unwrap();
    let mut id3 = TagBlockContent::default();
    id3.tags.insert(TagField::Title, "Old Title".to_string());
    id3.tags.insert(TagField::Artist, "Someone".to_string());
    TagEngine::write_block(&path, TagBlockKind::Id3v2, None, &id3).unwrap();

    let file = TagEngine::read(&path).expect("read");
    assert_eq!(file.tags.get(&TagField::Album), None);
    let ape_bytes = |path: &std::path::Path| {
        let bytes = std::fs::read(path).unwrap();
        let at = bytes
            .windows(8)
            .position(|window| window == b"APETAGEX")
            .expect("an APE block");
        bytes[at..].to_vec()
    };
    let untouched = ape_bytes(&path);
    TagEngine::write(&file).expect("a save that changes nothing");
    assert_eq!(ape_bytes(&path), untouched);

    retitle(&path, "New Title");
    let block = TagEngine::read_block(&path, TagBlockKind::Ape)
        .unwrap()
        .expect("the APE block stays");
    std::fs::remove_file(&path).ok();
    assert_eq!(
        block.tags.get(&TagField::Title).map(String::as_str),
        Some("New Title")
    );
    assert_eq!(
        block.tags.get(&TagField::Album).map(String::as_str),
        Some("Only In APE")
    );
    assert_eq!(block.tags.get(&TagField::Artist), None);
    assert_eq!(block.covers, content.covers);
}

/// #656: clearing every field took out of a legacy block only what the model
/// had read, so a field only the legacy block stated outlived the clear, and
/// with the ID3v2 tag now empty the next read came from the legacy block and
/// showed it again.
#[test]
fn clearing_every_field_leaves_no_legacy_field_behind() {
    use lofty::config::WriteOptions;
    use lofty::id3::v1::Id3v1Tag;
    use lofty::prelude::TagExt;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-clear-legacy-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).unwrap();
    Id3v1Tag {
        title: Some("Title".to_string()),
        album: Some("Only In ID3v1".to_string()),
        ..Default::default()
    }
    .save_to_path(&path, WriteOptions::default())
    .unwrap();
    let mut content = TagBlockContent::default();
    content.tags.insert(TagField::Title, "Title".to_string());
    content
        .tags
        .insert(TagField::Genre, "Only In APE".to_string());
    TagEngine::write_block(&path, TagBlockKind::Ape, None, &content).unwrap();
    let mut id3 = TagBlockContent::default();
    id3.tags.insert(TagField::Title, "Title".to_string());
    TagEngine::write_block(&path, TagBlockKind::Id3v2, None, &id3).unwrap();

    let mut file = TagEngine::read(&path).expect("read");
    assert_eq!(file.tags.len(), 1, "{:?}", file.tags);
    file.tags.clear();
    TagEngine::write(&file).expect("a clear");
    let read_back = TagEngine::read(&path).expect("read back");
    std::fs::remove_file(&path).ok();
    assert!(read_back.tags.is_empty(), "{:?}", read_back.tags);

    // A WAV's INFO list beside its ID3v2 chunk.
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-clear-legacy-{}.wav",
        std::process::id()
    ));
    std::fs::write(&path, minimal_wav()).unwrap();
    let mut info = lofty::iff::wav::RiffInfoList::new();
    info.insert("INAM".to_string(), "Title".to_string());
    info.insert("IGNR".to_string(), "Only In INFO".to_string());
    info.save_to_path(&path, WriteOptions::default()).unwrap();
    TagEngine::write_block(&path, TagBlockKind::Id3v2, None, &id3).unwrap();

    let mut file = TagEngine::read(&path).expect("read");
    assert_eq!(file.tags.len(), 1, "{:?}", file.tags);
    file.tags.clear();
    TagEngine::write(&file).expect("a clear");
    let read_back = TagEngine::read(&path).expect("read back");
    std::fs::remove_file(&path).ok();
    assert!(read_back.tags.is_empty(), "{:?}", read_back.tags);
}

/// #657: with the comment cleared, a second `COMM` the file carried under a
/// description of its own was kept, and the next read took it for the comment
/// (#644 reads the last one that is not a machine one when no frame lacks a
/// description), so the cleared comment came back.
#[test]
fn a_cleared_comment_does_not_come_back_from_a_described_frame() {
    let path = mp3_with_comments(
        "cleared-described",
        &[
            (b"XXX", "", "the comment"),
            (b"XXX", "ID3v1 Comment", "the comment"),
            (b"eng", "iTunSMPB", ITUNES_GAPLESS),
        ],
    );
    let mut file = TagEngine::read(&path).unwrap();
    assert_eq!(
        file.tags.get(&TagField::Comment).map(String::as_str),
        Some("the comment")
    );
    file.tags.remove(&TagField::Comment);
    TagEngine::write(&file).unwrap();

    let read_back = TagEngine::read(&path).unwrap();
    let frames = comment_frames(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(read_back.tags.get(&TagField::Comment), None);
    assert_eq!(
        frames,
        vec![(
            "eng".to_string(),
            "iTunSMPB".to_string(),
            ITUNES_GAPLESS.to_string()
        )],
        "the machine frame stays"
    );
}

/// #660: ID3v2.3 states a genre as a numeric reference refined by its name,
/// `(17)Rock`, and the backend reads the two halves as two genres.
#[test]
fn a_numeric_genre_with_its_name_reads_once() {
    use lofty::config::WriteOptions;
    use lofty::id3::v2::{Frame, FrameId, Id3v2Tag, TextInformationFrame};
    use lofty::prelude::TagExt;
    use lofty::TextEncoding;
    use std::borrow::Cow;

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-numeric-genre-{}.mp3",
        std::process::id()
    ));
    for (raw, expected) in [
        ("(17)Rock", "Rock"),
        ("(17)rock", "Rock"),
        ("(4)(17)", "Disco; Rock"),
        ("(17)(4)Eurodisco", "Rock; Disco; Eurodisco"),
    ] {
        std::fs::write(&path, minimal_mp3()).unwrap();
        let mut tag = Id3v2Tag::new();
        for (id, value) in [("TIT2", "Title"), ("TCON", raw)] {
            tag.insert(Frame::Text(TextInformationFrame::new(
                FrameId::Valid(Cow::Borrowed(id)),
                TextEncoding::Latin1,
                value.to_string(),
            )));
        }
        tag.save_to_path(&path, WriteOptions::default().use_id3v23(true))
            .unwrap();
        let file = TagEngine::read(&path).unwrap();
        assert_eq!(
            file.tags.get(&TagField::Genre).map(String::as_str),
            Some(expected),
            "{raw}"
        );
        TagEngine::write(&file).unwrap();
        assert_eq!(
            TagEngine::read(&path)
                .unwrap()
                .tags
                .get(&TagField::Genre)
                .map(String::as_str),
            Some(expected),
            "{raw}, after a save"
        );
    }
    std::fs::remove_file(&path).ok();
}

/// #663: the backend has no APE item for the BPM or the key and dropped both on
/// save; they are written and read by name now.
#[test]
fn an_ape_tag_keeps_the_bpm_and_the_key() {
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-ape-bpm-key-{}.wv",
        std::process::id()
    ));
    std::fs::write(&path, MINIMAL_WAVPACK).unwrap();
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Title, "Title".to_string());
    tags.insert(TagField::Bpm, "128".to_string());
    tags.insert(TagField::InitialKey, "8A".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::WavPack,
        tags: tags.clone(),
    })
    .unwrap();
    let read = TagEngine::read(&path).unwrap();
    let block = TagEngine::read_block(&path, TagBlockKind::Ape)
        .unwrap()
        .unwrap();

    // Cleared again, both go.
    let mut file = read.clone();
    file.tags.remove(&TagField::Bpm);
    file.tags.remove(&TagField::InitialKey);
    TagEngine::write(&file).unwrap();
    let cleared = TagEngine::read(&path).unwrap();
    std::fs::remove_file(&path).ok();

    assert_eq!(read.tags, tags);
    assert_eq!(block.tags, tags);
    assert_eq!(cleared.tags.get(&TagField::Bpm), None);
    assert_eq!(cleared.tags.get(&TagField::InitialKey), None);
    assert_eq!(cleared.tags.len(), 1, "{:?}", cleared.tags);
}

/// #664: an ID3v1 tag padded with spaces, as many encoders write it, came back
/// padded with NULs from a save that changed nothing in it.
#[test]
fn a_space_padded_id3v1_tag_survives_a_save_byte_for_byte() {
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-id3v1-spaces-{}.mp3",
        std::process::id()
    ));
    let field = |text: &str, width: usize| {
        let mut bytes = text.as_bytes().to_vec();
        bytes.resize(width, b' ');
        bytes
    };
    let mut legacy = b"TAG".to_vec();
    legacy.extend(field("Title", 30));
    legacy.extend(field("Artist", 30));
    legacy.extend(field("Album", 30));
    legacy.extend(b"1999");
    legacy.extend(field("Comment", 28));
    legacy.extend([0, 3, 17]);
    assert_eq!(legacy.len(), 128);
    let mut bytes = minimal_mp3();
    bytes.extend(&legacy);
    std::fs::write(&path, &bytes).unwrap();

    let file = TagEngine::read(&path).unwrap();
    TagEngine::write(&file).unwrap();
    let after = std::fs::read(&path).unwrap();
    let edited = retitle(&path, "New");
    let retitled = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).ok();

    assert_eq!(
        file.tags.get(&TagField::Title).map(String::as_str),
        Some("Title")
    );
    assert_eq!(&after[after.len() - 128..], &legacy[..]);
    // An edit rewrites the field it changed, and only that one.
    assert_eq!(
        edited.tags.get(&TagField::Title).map(String::as_str),
        Some("New")
    );
    let tail = &retitled[retitled.len() - 128..];
    assert_eq!(
        &tail[33..],
        &legacy[33..],
        "the other fields keep their padding"
    );
}

/// #665: the backend hands an `ilst`'s atoms over in no fixed order, so every
/// save of an M4A moved them around, even one that changed nothing.
#[test]
fn an_m4a_save_keeps_its_atoms_in_order() {
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-m4a-order-{}.m4a",
        std::process::id()
    ));
    std::fs::write(&path, MINIMAL_M4A).unwrap();
    let mut tags = BTreeMap::new();
    for (field, value) in [
        (TagField::Title, "Title"),
        (TagField::Artist, "Artist"),
        (TagField::Album, "Album"),
        (TagField::AlbumArtist, "Album Artist"),
        (TagField::Genre, "House"),
        (TagField::Year, "2001"),
        (TagField::Composer, "Composer"),
        (TagField::Comment, "Comment"),
        (TagField::Bpm, "124"),
        (TagField::Custom("CATALOG".to_string()), "CAT-1"),
        (TagField::Custom("LABEL_URL".to_string()), "x"),
    ] {
        tags.insert(field, value.to_string());
    }
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::M4a,
        tags,
    })
    .unwrap();
    // The atoms a store purchase carries, none of which the model holds: it is
    // these that came back in a different order on every save.
    {
        use lofty::config::{ParseOptions, WriteOptions};
        use lofty::file::AudioFile;
        use lofty::mp4::{Atom, AtomData, AtomIdent, Mp4File};
        use lofty::prelude::TagExt;

        let mut ilst = Mp4File::read_from(
            &mut std::fs::File::open(&path).unwrap(),
            ParseOptions::new(),
        )
        .unwrap()
        .ilst()
        .cloned()
        .unwrap();
        for (ident, value) in [
            (*b"apID", "someone@example.com"),
            (*b"purd", "2020-01-01 00:00:00"),
            (*b"xid ", "label:isrc:XX0000000000"),
        ] {
            ilst.insert(Atom::new(
                AtomIdent::Fourcc(ident),
                AtomData::UTF8(value.to_string()),
            ));
        }
        for (index, ident) in [*b"cnID", *b"plID", *b"atID", *b"cmID", *b"geID", *b"sfID"]
            .into_iter()
            .enumerate()
        {
            ilst.insert(Atom::new(
                AtomIdent::Fourcc(ident),
                AtomData::SignedInteger(i32::try_from(index).unwrap() + 1),
            ));
        }
        for ident in [*b"pgap", *b"cpil"] {
            ilst.insert(Atom::new(AtomIdent::Fourcc(ident), AtomData::Bool(true)));
        }
        for ident in [*b"stik", *b"rtng"] {
            ilst.insert(Atom::new(
                AtomIdent::Fourcc(ident),
                AtomData::SignedInteger(1),
            ));
        }
        ilst.save_to_path(&path, WriteOptions::default()).unwrap();
    }
    let mut saves = Vec::new();
    for _ in 0..4 {
        let file = TagEngine::read(&path).unwrap();
        TagEngine::write(&file).unwrap();
        saves.push(std::fs::read(&path).unwrap());
    }
    std::fs::remove_file(&path).ok();
    assert!(saves.windows(2).all(|pair| pair[0] == pair[1]));
}

/// The frames of `path`'s ID3v2 block as (id, text) pairs, the text empty for a
/// frame that holds none.
fn id3v2_frames(path: &std::path::Path) -> Vec<(String, String)> {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::id3::v2::Frame;

    let mut file = std::fs::File::open(path).unwrap();
    let tag = if path.extension().is_some_and(|ext| ext == "aiff") {
        lofty::iff::aiff::AiffFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .id3v2()
            .cloned()
    } else {
        lofty::mpeg::MpegFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .id3v2()
            .cloned()
    }
    .unwrap();
    (&tag)
        .into_iter()
        .map(|frame| {
            let text = match frame {
                Frame::Text(text) => text.value.to_string(),
                Frame::Timestamp(timestamp) => timestamp.timestamp.to_string(),
                _ => String::new(),
            };
            (frame.id().as_str().to_string(), text)
        })
        .collect()
}

/// The frame ids of `path`'s ID3v2.3 block, walked from its bytes: an MP3's
/// at the head of the file, an AIFF's in its `ID3 ` chunk.
fn raw_v23_frame_ids(path: &std::path::Path) -> Vec<String> {
    let data = std::fs::read(path).unwrap();
    let block = if data.starts_with(b"FORM") {
        let mut pos = 12;
        loop {
            let size = u32::from_be_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
            if &data[pos..pos + 4] == b"ID3 " {
                break &data[pos + 8..pos + 8 + size];
            }
            pos += 8 + size + (size & 1);
        }
    } else {
        &data[..]
    };
    assert_eq!(&block[..4], b"ID3\x03", "not an ID3v2.3 block");
    let size = block[6..10]
        .iter()
        .fold(0usize, |acc, byte| (acc << 7) | usize::from(byte & 0x7F));
    let mut ids = Vec::new();
    let mut pos = 10;
    while pos + 10 <= 10 + size && block[pos] != 0 {
        ids.push(String::from_utf8_lossy(&block[pos..pos + 4]).into_owned());
        pos += 10 + u32::from_be_bytes(block[pos + 4..pos + 8].try_into().unwrap()) as usize;
    }
    ids
}

/// #667: a 2.3 write keeps the frames the backend would drop as 2.4-only, the
/// sort orders and the release time among them, on an MP3 and in an AIFF.
#[test]
fn a_v23_write_keeps_the_sort_order_and_release_time_frames() {
    let frames = [
        id3v23_frame(b"TIT2", b"\0Title"),
        id3v23_frame(b"TSOA", b"\0Sort Album"),
        id3v23_frame(b"TSOP", b"\0Sort Artist"),
        id3v23_frame(b"TDRL", b"\x002022-03-25"),
    ]
    .concat();
    let tag = id3v23_tag(&frames);

    let dir = std::env::temp_dir().join(format!("tagrex-v23-drop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mp3 = dir.join("track.mp3");
    std::fs::write(&mp3, [tag.clone(), minimal_mp3()].concat()).unwrap();
    let aiff = dir.join("track.aiff");
    let mut aiff_bytes = minimal_aiff();
    aiff_bytes.extend_from_slice(b"ID3 ");
    aiff_bytes.extend_from_slice(&(tag.len() as u32).to_be_bytes());
    aiff_bytes.extend_from_slice(&tag);
    if tag.len() % 2 == 1 {
        aiff_bytes.push(0);
    }
    let form_size = (aiff_bytes.len() - 8) as u32;
    aiff_bytes[4..8].copy_from_slice(&form_size.to_be_bytes());
    std::fs::write(&aiff, aiff_bytes).unwrap();

    for path in [&mp3, &aiff] {
        assert!(TagEngine::set_id3v2_revision(path, Id3v2Revision::V3).unwrap());
        assert_eq!(
            TagEngine::id3v2_revision(path).unwrap(),
            Some(Id3v2Revision::V3),
            "{path:?}"
        );
        let frames = id3v2_frames(path);
        for expected in [
            ("TIT2", "Title"),
            ("TSOA", "Sort Album"),
            ("TSOP", "Sort Artist"),
            ("TDRL", "2022-03-25"),
        ] {
            assert!(
                frames
                    .iter()
                    .any(|(id, text)| id == expected.0 && text == expected.1),
                "{path:?} lost {expected:?}: {frames:?}"
            );
            // Exactly once on disk: should the backend stop dropping the
            // frame, the splice must not write it a second time (#671). The
            // count is taken from the bytes, since a read folds a repeated text
            // frame into one.
            let on_disk = raw_v23_frame_ids(path);
            assert_eq!(
                on_disk.iter().filter(|id| *id == expected.0).count(),
                1,
                "{path:?} has {} more than once: {on_disk:?}",
                expected.0
            );
        }
        // And a second write finds them where the first put them.
        assert!(TagEngine::set_id3v2_revision(path, Id3v2Revision::V3).unwrap());
        assert_eq!(id3v2_frames(path).len(), frames.len(), "{path:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// #668: a 2.3 `TYER` holding a full date was dropped by the parse, so the year
/// never showed and the next save removed it. It reads as the year now, and an
/// edit or a revision switch writes it back.
#[test]
fn a_v23_year_holding_a_full_date_is_read_and_kept() {
    let tag = id3v23_tag(
        &[
            id3v23_frame(b"TIT2", b"\0Title"),
            id3v23_frame(b"TYER", b"\x002021-09-08"),
        ]
        .concat(),
    );
    let dir = std::env::temp_dir().join(format!("tagrex-v23-tyer-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mp3 = dir.join("track.mp3");
    std::fs::write(&mp3, [tag.clone(), minimal_mp3()].concat()).unwrap();
    let aiff = dir.join("track.aiff");
    let mut aiff_bytes = minimal_aiff();
    aiff_bytes.extend_from_slice(b"ID3 ");
    aiff_bytes.extend_from_slice(&(tag.len() as u32).to_be_bytes());
    aiff_bytes.extend_from_slice(&tag);
    if tag.len() % 2 == 1 {
        aiff_bytes.push(0);
    }
    let form_size = (aiff_bytes.len() - 8) as u32;
    aiff_bytes[4..8].copy_from_slice(&form_size.to_be_bytes());
    std::fs::write(&aiff, aiff_bytes).unwrap();

    let year = |path: &std::path::Path| {
        TagEngine::read(path)
            .unwrap()
            .tags
            .get(&TagField::Year)
            .cloned()
    };
    for path in [&mp3, &aiff] {
        assert_eq!(year(path).as_deref(), Some("2021-09-08"), "{path:?}");

        // A revision switch rewrites the block from the concrete read.
        assert!(TagEngine::set_id3v2_revision(path, Id3v2Revision::V3).unwrap());
        assert_eq!(year(path).as_deref(), Some("2021-09-08"), "{path:?}");

        // An edit of another field.
        let mut file = TagEngine::read(path).unwrap();
        file.tags.insert(TagField::Title, "Edited".to_string());
        TagEngine::write(&file).unwrap();
        assert_eq!(year(path).as_deref(), Some("2021-09-08"), "{path:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// #670: a genre ID3v1's list has no index for left the ID3v1 genre byte as
/// "none" on every save, even one that never touched the genre. The byte stays
/// while the genre does; an edit writes the new genre's index, or "none".
#[test]
fn an_unedited_genre_outside_the_id3v1_list_keeps_its_byte() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v1::Id3v1Tag;
    use lofty::id3::v2::Id3v2Tag;
    use lofty::mpeg::MpegFile;
    use lofty::prelude::{Accessor, TagExt};

    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-id3v1-genre-{}.mp3",
        std::process::id()
    ));
    std::fs::write(&path, minimal_mp3()).expect("write fixture");
    let mut id3v2 = Id3v2Tag::new();
    id3v2.set_title("Title".to_string());
    id3v2.set_genre("Speed Garage".to_string());
    id3v2
        .save_to_path(&path, WriteOptions::default())
        .expect("seed id3v2");
    Id3v1Tag {
        title: Some("Title".to_string()),
        genre: Some(12),
        ..Default::default()
    }
    .save_to_path(&path, WriteOptions::default())
    .expect("seed id3v1");
    let id3v1_genre = || {
        let mut handle = std::fs::File::open(&path).unwrap();
        MpegFile::read_from(&mut handle, ParseOptions::new())
            .unwrap()
            .id3v1()
            .cloned()
            .expect("ID3v1 kept")
            .genre
    };
    let save = |field: TagField, value: &str| {
        let mut file = TagEngine::read(&path).expect("read");
        file.tags.insert(field, value.to_string());
        TagEngine::write(&file).expect("write");
    };

    save(TagField::Title, "New title");
    assert_eq!(id3v1_genre(), Some(12), "the genre was not edited");

    save(TagField::Genre, "Rock");
    assert_eq!(id3v1_genre(), Some(17), "a listed genre is kept in step");

    save(TagField::Genre, "UK Garage");
    assert_eq!(id3v1_genre(), None, "an edit to an unlisted genre");

    std::fs::remove_file(&path).ok();
}

/// The order of a multi-value field is the file's: the first artist is the main
/// one. Writing the read-back tags with only the title changed used to hand the
/// artists back reversed, on the first write and on every one after it.
#[test]
fn a_multi_value_order_survives_an_edit_of_another_field() {
    let path = temp_flac_path("multi-value-order");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");

    // A file that held one artist first, then was given two: the history the
    // order went wrong on.
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "placeholder".to_string());
    tags.insert(TagField::Title, "Old".to_string());
    TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags,
    })
    .expect("seed");
    let mut seeded = TagEngine::read(&path).expect("read seed");
    seeded
        .tags
        .insert(TagField::Artist, "Autechre; Gescom".to_string());
    seeded
        .tags
        .insert(TagField::Genre, "Electronic; IDM; Ambient".to_string());
    // A custom comment too: the stale custom items are taken out on every save.
    seeded.tags.insert(
        TagField::Custom("TAGREX_ORDER".to_string()),
        "x".to_string(),
    );
    TagEngine::write(&seeded).expect("seed multi-value");

    let mut seen = Vec::new();
    for title in ["New", "Newer", "Newest"] {
        let mut file = TagEngine::read(&path).expect("read");
        seen.push((
            file.tags.get(&TagField::Artist).cloned(),
            file.tags.get(&TagField::Genre).cloned(),
        ));
        file.tags.insert(TagField::Title, title.to_string());
        TagEngine::write(&file).expect("write");
    }
    let last = TagEngine::read(&path).expect("read");
    seen.push((
        last.tags.get(&TagField::Artist).cloned(),
        last.tags.get(&TagField::Genre).cloned(),
    ));
    std::fs::remove_file(&path).ok();

    for (artist, genre) in seen {
        assert_eq!(artist.as_deref(), Some("Autechre; Gescom"));
        assert_eq!(genre.as_deref(), Some("Electronic; IDM; Ambient"));
    }
}

/// Every save takes the `ENCODER` comment out and puts it back, and the tag
/// library's removal reorders what it leaves: a FLAC with an encoder comment
/// came back with its artists reversed after an edit of the title alone.
#[test]
fn a_multi_value_order_survives_a_save_of_a_file_with_an_encoder_comment() {
    let path = temp_flac_path("multi-value-encoder");
    // A Vorbis comment block as an encoder writes it, built by hand: the model
    // folds an `ENCODER` comment into the vendor string, so no write of ours
    // can make one.
    let comments: [&[u8]; 4] = [
        b"ENCODER=flac 1.4.3",
        b"ARTIST=Autechre",
        b"ARTIST=Gescom",
        b"TITLE=Old",
    ];
    let mut block = Vec::new();
    block.extend_from_slice(&(b"reference".len() as u32).to_le_bytes());
    block.extend_from_slice(b"reference");
    block.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for comment in comments {
        block.extend_from_slice(&(comment.len() as u32).to_le_bytes());
        block.extend_from_slice(comment);
    }
    let mut bytes = MINIMAL_FLAC[..42].to_vec(); // fLaC + STREAMINFO
    bytes.push(0x04); // VORBIS_COMMENT, not last
    bytes.extend_from_slice(&(block.len() as u32).to_be_bytes()[1..]);
    bytes.extend_from_slice(&block);
    bytes.extend_from_slice(&MINIMAL_FLAC[42..]); // PADDING (last) + audio
    std::fs::write(&path, &bytes).expect("write fixture");
    let seeded = TagEngine::read(&path).expect("read fixture");
    assert_eq!(
        seeded.tags.get(&TagField::Artist).map(String::as_str),
        Some("Autechre; Gescom"),
        "fixture precondition"
    );

    let mut file = TagEngine::read(&path).expect("read");
    file.tags.insert(TagField::Title, "New".to_string());
    TagEngine::write(&file).expect("write");
    let after = TagEngine::read(&path).expect("read back");
    let raw = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).ok();

    assert!(
        raw.windows(7).any(|w| w.eq_ignore_ascii_case(b"ENCODER")),
        "fixture precondition: the file holds an encoder comment"
    );
    assert_eq!(
        after.tags.get(&TagField::Artist).map(String::as_str),
        Some("Autechre; Gescom")
    );
}

// [2026-11-03 14:10] Added: regression test for cp1251 text in an unedited Latin-1 frame.
/// Older Russian and Ukrainian files carry cp1251 bytes in Latin-1 frames: read
/// here as `Êèíî` for `Кино`. An edit of another field rebuilt every frame in
/// Unicode and wrote that mojibake back for good, so every player and tagger
/// that showed the title right before showed garbage after. An unedited frame
/// keeps its encoding, and so its bytes; an edited one is written as before.
#[test]
fn an_unedited_frame_keeps_its_encoding_and_its_bytes() {
    let path = std::env::temp_dir().join(format!(
        "tagrex-tag-engine-test-cp1251-{}.mp3",
        std::process::id()
    ));
    let kino_1251: &[u8] = &[0xCA, 0xE8, 0xED, 0xEE];
    let frame = |id: &[u8], encoding: u8, text: &[u8]| {
        let mut f = id.to_vec();
        f.extend_from_slice(&((text.len() + 1) as u32).to_be_bytes());
        f.extend_from_slice(&[0, 0, encoding]);
        f.extend_from_slice(text);
        f
    };
    let tsoy_utf16: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain("Цой".encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    let body = [frame(b"TIT2", 0, kino_1251), frame(b"TPE1", 1, &tsoy_utf16)].concat();
    let size = body.len() as u32;
    let mut bytes = b"ID3\x03\x00\x00".to_vec();
    bytes.extend_from_slice(&[
        ((size >> 21) & 0x7f) as u8,
        ((size >> 14) & 0x7f) as u8,
        ((size >> 7) & 0x7f) as u8,
        (size & 0x7f) as u8,
    ]);
    bytes.extend_from_slice(&body);
    for _ in 0..20 {
        let mut audio = vec![0u8; 417];
        audio[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        bytes.extend_from_slice(&audio);
    }
    std::fs::write(&path, &bytes).unwrap();

    let mut file = TagEngine::read(&path).unwrap();
    assert_eq!(
        file.tags.get(&TagField::Title).map(String::as_str),
        Some("Êèíî"),
        "fixture precondition: read as Latin-1"
    );
    file.tags.insert(TagField::Genre, "Rock".to_string());
    TagEngine::write(&file).unwrap();
    let raw = std::fs::read(&path).unwrap();
    let contains = |needle: &[u8]| raw.windows(needle.len()).any(|w| w == needle);

    // The title frame is still Latin-1 (encoding byte 0) holding the cp1251 bytes.
    let mut title_frame = vec![0u8];
    title_frame.extend_from_slice(kino_1251);
    assert!(contains(&title_frame), "the cp1251 title kept its bytes");
    assert!(contains(&tsoy_utf16), "the UTF-16 artist kept its encoding");

    // An edited value is written as before, readable as what was typed.
    let mut edited = TagEngine::read(&path).unwrap();
    edited.tags.insert(TagField::Title, "Кино".to_string());
    TagEngine::write(&edited).unwrap();
    let after = TagEngine::read(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(
        after.tags.get(&TagField::Title).map(String::as_str),
        Some("Кино")
    );
    assert_eq!(
        after.tags.get(&TagField::Artist).map(String::as_str),
        Some("Цой")
    );
}
