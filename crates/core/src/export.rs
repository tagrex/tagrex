//! Exporters: playlist, CSV and text-report renderers.
//!
//! Pure string builders — they never touch the filesystem, so the callers
//! (the app command layer) own reading tracks and writing the result. Exporting
//! is read-only with respect to the audio files: nothing here goes through the
//! [`Executor`](crate::plan::Executor) pipeline, because nothing is modified.

use crate::mask::Mask;
use crate::model::{TagField, TagMap, TrackFile};

/// One playlist entry. `path` is written verbatim, so the caller decides
/// whether it is relative (portable, next to the playlist) or absolute.
pub struct PlaylistTrack {
    pub path: String,
    pub artist: String,
    pub title: String,
    /// Track length in whole seconds; `-1` when unknown (the M3U convention).
    pub duration_secs: i64,
}

/// Extended M3U (`#EXTM3U` + `#EXTINF` per entry), the format every player
/// understands.
pub fn m3u(tracks: &[PlaylistTrack]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for track in tracks {
        let label = if track.artist.is_empty() {
            track.title.clone()
        } else {
            format!("{} - {}", track.artist, track.title)
        };
        out.push_str(&format!(
            "#EXTINF:{},{}\n{}\n",
            track.duration_secs, label, track.path
        ));
    }
    out
}

/// Columns written by [`csv`], in order.
const CSV_COLUMNS: [(&str, Option<TagField>); 11] = [
    ("File", None),
    ("Artist", Some(TagField::Artist)),
    ("Title", Some(TagField::Title)),
    ("Album", Some(TagField::Album)),
    ("Album Artist", Some(TagField::AlbumArtist)),
    ("Track", Some(TagField::TrackNumber)),
    ("Disc", Some(TagField::DiscNumber)),
    ("Year", Some(TagField::Year)),
    ("Genre", Some(TagField::Genre)),
    ("Comment", Some(TagField::Comment)),
    ("Path", None),
];

/// RFC 4180 CSV of the tag columns, with a header row.
pub fn csv(tracks: &[TrackFile]) -> String {
    let mut out = String::new();
    let header: Vec<String> = CSV_COLUMNS
        .iter()
        .map(|(name, _)| csv_field(name))
        .collect();
    out.push_str(&header.join(","));
    out.push_str("\r\n");

    for track in tracks {
        let path = track.path.to_string_lossy();
        let file_name = track
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let row: Vec<String> = CSV_COLUMNS
            .iter()
            .map(|(name, field)| match field {
                Some(field) => csv_field(track.tags.get(field).map(String::as_str).unwrap_or("")),
                // The two path columns are positional, not tag-backed.
                None if *name == "File" => csv_field(&file_name),
                None => csv_field(&path),
            })
            .collect();
        out.push_str(&row.join(","));
        out.push_str("\r\n");
    }
    out
}

/// Quote a CSV field when it contains a separator, quote or newline, doubling
/// any embedded quotes.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// One rendered line per track from a mask template (the same placeholder
/// syntax as rename masks, e.g. `%artist% - %title%`).
///
/// Rendering is lenient: a placeholder whose tag is missing becomes an empty
/// string rather than dropping the whole line, so a report always covers every
/// track it was given.
pub fn report(tracks: &[TrackFile], mask: &Mask) -> String {
    let mut out = String::new();
    for track in tracks {
        if let Ok(line) = mask.render(&lenient_tags(&track.tags)) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// A tag map with every well-known field present (empty unless the track sets
/// it), so [`Mask::render`] can't fail with `MissingTag`.
fn lenient_tags(tags: &TagMap) -> TagMap {
    let mut lenient = TagMap::new();
    for field in [
        TagField::Artist,
        TagField::Title,
        TagField::Album,
        TagField::AlbumArtist,
        TagField::TrackNumber,
        TagField::TrackTotal,
        TagField::DiscNumber,
        TagField::Year,
        TagField::Genre,
        TagField::Comment,
        TagField::Composer,
        TagField::Publisher,
        TagField::Bpm,
        TagField::Isrc,
        TagField::InitialKey,
        TagField::CatalogNumber,
    ] {
        lenient.insert(field, String::new());
    }
    for (field, value) in tags {
        lenient.insert(field.clone(), value.clone());
    }
    lenient
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AudioFormat;
    use std::path::PathBuf;

    fn track(path: &str, pairs: &[(TagField, &str)]) -> TrackFile {
        let mut tags = TagMap::new();
        for (field, value) in pairs {
            tags.insert(field.clone(), (*value).to_string());
        }
        TrackFile {
            path: PathBuf::from(path),
            format: AudioFormat::Mp3,
            tags,
        }
    }

    #[test]
    fn m3u_has_header_and_extinf_per_entry() {
        let out = m3u(&[
            PlaylistTrack {
                path: "01 - a.mp3".into(),
                artist: "The X Factor".into(),
                title: "Desert Rain".into(),
                duration_secs: 278,
            },
            // No artist -> the label is just the title; unknown length -> -1.
            PlaylistTrack {
                path: "02 - b.mp3".into(),
                artist: String::new(),
                title: "Radio".into(),
                duration_secs: -1,
            },
        ]);
        assert_eq!(
            out,
            "#EXTM3U\n\
             #EXTINF:278,The X Factor - Desert Rain\n\
             01 - a.mp3\n\
             #EXTINF:-1,Radio\n\
             02 - b.mp3\n"
        );
    }

    #[test]
    fn csv_writes_header_and_quotes_special_characters() {
        let out = csv(&[track(
            "/music/x.mp3",
            &[
                (TagField::Artist, "Tom, Dick"),
                (TagField::Title, "He said \"hi\""),
                (TagField::Year, "1996"),
            ],
        )]);
        let mut lines = out.split("\r\n");
        assert_eq!(
            lines.next().unwrap(),
            "File,Artist,Title,Album,Album Artist,Track,Disc,Year,Genre,Comment,Path"
        );
        assert_eq!(
            lines.next().unwrap(),
            "x.mp3,\"Tom, Dick\",\"He said \"\"hi\"\"\",,,,,1996,,,/music/x.mp3"
        );
    }

    #[test]
    fn report_renders_a_line_per_track_and_drops_empty_optional_parts() {
        // The album sits in a conditional section, so a track without one gets
        // no stray "()" — the section disappears along with its separator.
        let mask = Mask::parse("%artist% - %title%[ (%album%)]").unwrap();
        let out = report(
            &[
                track(
                    "/music/a.mp3",
                    &[
                        (TagField::Artist, "Plastic"),
                        (TagField::Title, "Sexy Groove"),
                        (TagField::Album, "La Bush"),
                    ],
                ),
                // Album missing: the section drops, the line stays.
                track(
                    "/music/b.mp3",
                    &[
                        (TagField::Artist, "B.B.E."),
                        (TagField::Title, "Seven Days"),
                    ],
                ),
            ],
            &mask,
        );
        assert_eq!(
            out,
            "Plastic - Sexy Groove (La Bush)\nB.B.E. - Seven Days\n"
        );
    }
}
