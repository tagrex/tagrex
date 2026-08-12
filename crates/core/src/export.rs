//! Exporters: playlist, CSV and text-report renderers.
//!
//! Pure string builders — they never touch the filesystem, so the callers
//! (the app command layer) own reading tracks and writing the result. Exporting
//! is read-only with respect to the audio files: nothing here goes through the
//! [`Executor`](crate::plan::Executor) pipeline, because nothing is modified.

use crate::mask::{FileContext, Mask};
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
        let row: Vec<String> = CSV_COLUMNS
            .iter()
            .map(|(name, field)| csv_field(&column_value(track, name, field)))
            .collect();
        out.push_str(&row.join(","));
        out.push_str("\r\n");
    }
    out
}

/// The value for one export column of one track: a tag value, or — for the two
/// positional path columns — the file name / full path. Shared by every
/// column-based exporter (CSV, HTML, XML).
fn column_value(track: &TrackFile, name: &str, field: &Option<TagField>) -> String {
    match field {
        Some(field) => track.tags.get(field).cloned().unwrap_or_default(),
        None if name == "File" => track
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        None => track.path.to_string_lossy().into_owned(),
    }
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

/// A self-contained HTML document (#42): a styled table of the tag columns, one
/// row per track. Standalone — inline `<style>`, no external assets — so it opens
/// and reads in any browser. Every value is HTML-escaped.
pub fn html(tracks: &[TrackFile]) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<title>TagRex export</title>\n<style>\n");
    out.push_str(
        "body{font:14px/1.5 system-ui,sans-serif;margin:2rem;color:#1a1a1a}\
         h1{font-size:1.2rem;margin:0 0 .25rem}\
         p.count{color:#666;margin:0 0 1rem}\
         table{border-collapse:collapse;width:100%;font-size:13px}\
         th,td{border:1px solid #ddd;padding:4px 8px;text-align:left;vertical-align:top}\
         th{background:#f4f4f5;position:sticky;top:0}\
         tr:nth-child(even) td{background:#fafafa}\n",
    );
    out.push_str("</style>\n</head>\n<body>\n");
    out.push_str("<h1>TagRex export</h1>\n");
    out.push_str(&format!(
        "<p class=\"count\">{} track{}</p>\n",
        tracks.len(),
        if tracks.len() == 1 { "" } else { "s" }
    ));
    out.push_str("<table>\n<thead>\n<tr>");
    for (name, _) in CSV_COLUMNS {
        out.push_str(&format!("<th>{}</th>", html_escape(name)));
    }
    out.push_str("</tr>\n</thead>\n<tbody>\n");
    for track in tracks {
        out.push_str("<tr>");
        for (name, field) in &CSV_COLUMNS {
            out.push_str(&format!(
                "<td>{}</td>",
                html_escape(&column_value(track, name, field))
            ));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody>\n</table>\n</body>\n</html>\n");
    out
}

/// An XML document (#42): `<library>` of `<track>` elements, one child element
/// per non-empty tag column (plus `file` and `path`, always present). Element
/// names are the lower-cased column names with spaces as underscores; every
/// value is XML-escaped.
pub fn xml(tracks: &[TrackFile]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!("<library count=\"{}\">\n", tracks.len()));
    for track in tracks {
        out.push_str("  <track>\n");
        for (name, field) in &CSV_COLUMNS {
            let value = column_value(track, name, field);
            // Skip empty *tag* columns, but always emit the positional File/Path.
            if value.is_empty() && field.is_some() {
                continue;
            }
            let element = xml_element_name(name);
            out.push_str(&format!(
                "    <{element}>{}</{element}>\n",
                xml_escape(&value)
            ));
        }
        out.push_str("  </track>\n");
    }
    out.push_str("</library>\n");
    out
}

/// Escape the five XML/HTML-significant characters for element/text content.
fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Escape XML text content (`&`, `<`, `>` are the ones that matter there).
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A column name as an XML element name: lower-cased, spaces to underscores
/// (`Album Artist` -> `album_artist`). The `CSV_COLUMNS` names are ASCII words,
/// so the result is always a valid element name.
fn xml_element_name(name: &str) -> String {
    name.to_ascii_lowercase().replace(' ', "_")
}

/// One rendered line per track from a mask template (the same placeholder
/// syntax as rename masks, e.g. `%artist% - %title%`).
///
/// Rendering is lenient: a placeholder whose tag is missing becomes an empty
/// string rather than dropping the whole line, so a report always covers every
/// track it was given. File and technical placeholders (#147) resolve too — a
/// report is the obvious place to want `%_length%` or `%_bitrate%`.
pub fn report(tracks: &[TrackFile], mask: &Mask) -> String {
    let mut out = String::new();
    for track in tracks {
        let file = FileContext::read(mask, track);
        if let Ok(line) = mask.render_with(&lenient_tags(&track.tags), &file) {
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

    #[test]
    fn html_is_a_self_contained_document_and_escapes_values() {
        let out = html(&[track(
            "/music/x.mp3",
            &[
                (TagField::Artist, "Tom & <Jerry>"),
                (TagField::Title, "Hi"),
                (TagField::Year, "1996"),
            ],
        )]);
        assert!(out.starts_with("<!doctype html>"));
        assert!(out.contains("<style>")); // no external assets
        assert!(out.contains("<th>Album Artist</th>"));
        assert!(out.contains("<p class=\"count\">1 track</p>"));
        // Values are HTML-escaped; the file name column carries the base name.
        assert!(out.contains("<td>Tom &amp; &lt;Jerry&gt;</td>"));
        assert!(out.contains("<td>x.mp3</td>"));
        assert!(out.contains("<td>/music/x.mp3</td>"));
    }

    #[test]
    fn xml_emits_a_track_per_row_skipping_empty_tags_and_escaping() {
        let out = xml(&[track(
            "/music/x.mp3",
            &[
                (TagField::Artist, "Tom & Jerry"),
                (TagField::AlbumArtist, "V/A"),
                (TagField::Year, "1996"),
            ],
        )]);
        assert!(out.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
        assert!(out.contains("<library count=\"1\">"));
        assert!(out.contains("<file>x.mp3</file>"));
        assert!(out.contains("<path>/music/x.mp3</path>"));
        // Spaces in a column name become underscores; text is XML-escaped.
        assert!(out.contains("<album_artist>V/A</album_artist>"));
        assert!(out.contains("<artist>Tom &amp; Jerry</artist>"));
        // An empty tag column (e.g. Title here) is omitted entirely.
        assert!(!out.contains("<title>"));
    }
}
