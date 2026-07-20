//! Bidirectional mask engine.
//!
//! One grammar, two directions — this is an invariant, not a preference
//! (architecture.md). The same pattern string, e.g. `%artist% - %title%`,
//! must both *render* a filename from tags and *extract* tags from a
//! filename. A single implementation for both directions is mandatory:
//! divergent placeholder behavior between rename and import is the worst
//! class of bug this tool can have.
//!
//! Both directions are derived from the same parsed [`Segment`] list:
//! `render` substitutes it, `extract` compiles it into one anchored regex
//! (literals escaped, placeholders as capture groups) and matches against
//! it. There's no second, hand-rolled matcher to drift out of sync.
//!
//! Grammar beyond plain placeholders:
//! - `%field%` / `%field:width%` — a value, optionally zero-padded (track
//!   numbers pad to two digits by default).
//! - `[...]` — a conditional section, kept only when a placeholder inside it
//!   resolved to something. This is what lets one mask serve a library where
//!   some albums have a year and some don't, without emitting stray separators.
//! - `'x'` — a literal, for the reserved characters `% [ ]`; `''` is one quote.
//!
//! Only the first-class [`TagField`] variants are valid placeholder names —
//! `Custom` fields aren't addressable from a mask yet. Deferred rather than
//! ignored, same as scripting in architecture.md.

use std::borrow::Cow;

use regex::Regex;
use thiserror::Error;

use crate::model::{TagField, TagMap};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(String),
    /// A field plus the minimum width it renders to, zero-padded. Width `1`
    /// means "print it as-is".
    Placeholder(TagField, usize),
    /// A conditional section, `[...]`. Rendered only when at least one
    /// placeholder inside it resolves to a non-empty value, and dropped whole
    /// otherwise — which is what lets ` [%artist% - ]` contribute nothing (not
    /// even its space) on a single-artist album.
    Section(Vec<Segment>),
}

/// A parsed, validated mask pattern.
#[derive(Debug, Clone)]
pub struct Mask {
    pattern: String,
    segments: Vec<Segment>,
    regex: Regex,
    /// Two placeholders with nothing between them. Rendering them is perfectly
    /// well-defined (`%disc%%track%` -> `101`); *extracting* them is not, since
    /// nothing says where one value ends and the next begins. So this is only
    /// an error for the extract direction, not for the pattern as such.
    adjacent_placeholders: bool,
}

impl Mask {
    /// Parse and validate a pattern string.
    pub fn parse(pattern: &str) -> Result<Self, MaskError> {
        let segments = parse_segments(pattern)?;
        let mut previous_was_placeholder = false;
        let adjacent_placeholders =
            has_adjacent_placeholders(&segments, &mut previous_was_placeholder);
        let regex = build_regex(&segments);
        Ok(Self {
            pattern: pattern.to_string(),
            segments,
            regex,
            adjacent_placeholders,
        })
    }

    /// Tags -> filename (the Music Renamer direction).
    pub fn render(&self, tags: &TagMap) -> Result<String, MaskError> {
        let mut out = String::new();
        render_segments(&self.segments, tags, false, &mut out)?;
        Ok(out)
    }

    /// Filename -> tags (the import direction).
    pub fn extract(&self, filename: &str) -> Result<TagMap, MaskError> {
        // Rendering adjacent placeholders is fine; splitting the result back
        // apart is guesswork, so refuse rather than invent a boundary.
        if self.adjacent_placeholders {
            return Err(MaskError::Ambiguous);
        }
        let captures = self.regex.captures(filename).ok_or(MaskError::NoMatch)?;

        let mut tags = TagMap::new();
        let mut index = 0;
        collect_captures(&self.segments, &captures, &mut index, &mut tags);
        Ok(tags)
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }
}

fn parse_segments(pattern: &str) -> Result<Vec<Segment>, MaskError> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut position = 0;
    let segments = parse_until(&chars, &mut position, None)?;
    debug_assert_eq!(position, chars.len());
    Ok(segments)
}

/// Parse segments until `terminator` (or end of input when `None`).
///
/// Recursive because sections nest; the terminator distinguishes "ran out of
/// input at the top level", which is fine, from "ran out inside a section",
/// which is an unbalanced bracket.
fn parse_until(
    chars: &[char],
    position: &mut usize,
    terminator: Option<char>,
) -> Result<Vec<Segment>, MaskError> {
    let mut segments = Vec::new();
    let mut literal = String::new();

    while *position < chars.len() {
        let current = chars[*position];
        if Some(current) == terminator {
            *position += 1;
            flush_literal(&mut literal, &mut segments);
            return Ok(segments);
        }
        match current {
            '[' => {
                *position += 1;
                flush_literal(&mut literal, &mut segments);
                let inner = parse_until(chars, position, Some(']'))?;
                segments.push(Segment::Section(inner));
            }
            // Only reachable at the top level; inside a section `]` is the
            // terminator handled above.
            ']' => return Err(MaskError::UnbalancedSection),
            // Single quotes escape the reserved characters, so a pattern can
            // still contain a literal `%`, `[` or `]`. `''` yields one quote.
            '\'' => {
                *position += 1;
                let mut quoted = String::new();
                let mut closed = false;
                while *position < chars.len() {
                    if chars[*position] == '\'' {
                        *position += 1;
                        closed = true;
                        break;
                    }
                    quoted.push(chars[*position]);
                    *position += 1;
                }
                if !closed {
                    return Err(MaskError::UnterminatedQuote);
                }
                if quoted.is_empty() {
                    literal.push('\'');
                } else {
                    literal.push_str(&quoted);
                }
            }
            '%' => {
                *position += 1;
                let start = *position;
                while *position < chars.len() && chars[*position] != '%' {
                    *position += 1;
                }
                if *position >= chars.len() {
                    let rest: String = chars[start..].iter().collect();
                    return Err(MaskError::UnknownPlaceholder(rest));
                }
                let spec: String = chars[start..*position].iter().collect();
                *position += 1;
                flush_literal(&mut literal, &mut segments);
                segments.push(parse_placeholder(&spec)?);
            }
            other => {
                literal.push(other);
                *position += 1;
            }
        }
    }

    if terminator.is_some() {
        return Err(MaskError::UnbalancedSection);
    }
    flush_literal(&mut literal, &mut segments);
    Ok(segments)
}

fn flush_literal(literal: &mut String, segments: &mut Vec<Segment>) {
    if !literal.is_empty() {
        segments.push(Segment::Literal(std::mem::take(literal)));
    }
}

/// `name` or `name:width`, e.g. `%track:3%`.
fn parse_placeholder(spec: &str) -> Result<Segment, MaskError> {
    let (name, width) = match spec.split_once(':') {
        Some((name, width)) => (
            name,
            width
                .parse::<usize>()
                .map_err(|_| MaskError::UnknownPlaceholder(spec.to_string()))?,
        ),
        None => (spec, 0),
    };
    let field = field_from_name(name)?;
    let width = if width == 0 {
        default_width(&field)
    } else {
        width
    };
    Ok(Segment::Placeholder(field, width))
}

/// Every placeholder becomes a named, non-greedy, non-empty capture group;
/// every literal is escaped so its regex-special characters (common in
/// filenames: `.`, `(`, `)`, `[`, `]`) are matched literally. Building
/// always succeeds: literals are escaped and group names (`f0`, `f1`, ...)
/// are index-derived, so there's no way to produce invalid regex syntax or
/// a duplicate group name here.
fn build_regex(segments: &[Segment]) -> Regex {
    let mut pattern = String::from("^");
    let mut index = 0;
    build_regex_into(segments, &mut index, &mut pattern);
    pattern.push('$');
    Regex::new(&pattern).expect(
        "mask regex is built from escaped literals and indexed group names, so it always compiles",
    )
}

/// Group indices are assigned in pre-order, and [`collect_captures`] walks the
/// tree the same way, so the two stay in step without storing an index on the
/// segments themselves.
fn build_regex_into(segments: &[Segment], index: &mut usize, out: &mut String) {
    for segment in segments {
        match segment {
            Segment::Literal(text) => out.push_str(&regex::escape(text)),
            Segment::Placeholder(..) => {
                out.push_str(&format!("(?P<{}>.+?)", group_name(*index)));
                *index += 1;
            }
            // A conditional section is an optional group: the filename may or
            // may not carry that part.
            Segment::Section(inner) => {
                out.push_str("(?:");
                build_regex_into(inner, index, out);
                out.push_str(")?");
            }
        }
    }
}

fn collect_captures(
    segments: &[Segment],
    captures: &regex::Captures<'_>,
    index: &mut usize,
    tags: &mut TagMap,
) {
    for segment in segments {
        match segment {
            Segment::Literal(_) => {}
            Segment::Placeholder(field, _) => {
                if let Some(matched) = captures.name(&group_name(*index)) {
                    tags.insert(field.clone(), matched.as_str().to_string());
                }
                *index += 1;
            }
            Segment::Section(inner) => collect_captures(inner, captures, index, tags),
        }
    }
}

/// Render `segments` into `out`, returning whether any placeholder produced a
/// non-empty value.
///
/// `optional` is set inside a conditional section: there a missing tag simply
/// means the section contributes nothing, whereas outside one it is a genuinely
/// unsatisfiable pattern and stays an error.
fn render_segments(
    segments: &[Segment],
    tags: &TagMap,
    optional: bool,
    out: &mut String,
) -> Result<bool, MaskError> {
    let mut produced = false;
    for segment in segments {
        match segment {
            Segment::Literal(text) => out.push_str(text),
            Segment::Placeholder(field, width) => match tags.get(field) {
                Some(value) => {
                    let clean = sanitize_for_filename(value);
                    if !clean.is_empty() {
                        produced = true;
                    }
                    out.push_str(&pad_numeric(&clean, *width));
                }
                None if optional => {}
                None => return Err(MaskError::MissingTag(field_name(field).to_string())),
            },
            Segment::Section(inner) => {
                let mut buffer = String::new();
                if render_segments(inner, tags, true, &mut buffer)? {
                    out.push_str(&buffer);
                    produced = true;
                }
            }
        }
    }
    Ok(produced)
}

/// Two placeholders with no literal text between them, looking through section
/// boundaries — `[%disc%]%track%` is just as unsplittable as `%disc%%track%`.
fn has_adjacent_placeholders(segments: &[Segment], previous_was_placeholder: &mut bool) -> bool {
    for segment in segments {
        match segment {
            Segment::Literal(text) => {
                if !text.is_empty() {
                    *previous_was_placeholder = false;
                }
            }
            Segment::Placeholder(..) => {
                if *previous_was_placeholder {
                    return true;
                }
                *previous_was_placeholder = true;
            }
            Segment::Section(inner) => {
                if has_adjacent_placeholders(inner, previous_was_placeholder) {
                    return true;
                }
            }
        }
    }
    false
}

/// How wide a field renders by default.
///
/// Track numbers are conventionally zero-padded to two digits: it keeps a plain
/// alphabetical sort correct, and it is what makes a concatenated
/// `%disc%%track%` read as `101` (disc 1, track 01) instead of `11`, which a
/// player would take for track eleven. Everything else prints as-is; use an
/// explicit `%disc:2%` when a release needs it.
fn default_width(field: &TagField) -> usize {
    match field {
        TagField::TrackNumber => 2,
        _ => 1,
    }
}

/// Left-pad a purely numeric value with zeros to `width`. Anything that isn't
/// all digits (`A1`, `1/12`) is left alone — padding it would corrupt it.
fn pad_numeric(value: &str, width: usize) -> Cow<'_, str> {
    if width <= 1
        || value.is_empty()
        || value.len() >= width
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        Cow::Borrowed(value)
    } else {
        Cow::Owned(format!("{value:0>width$}"))
    }
}

fn group_name(index: usize) -> String {
    format!("f{index}")
}

fn field_from_name(name: &str) -> Result<TagField, MaskError> {
    match name.to_ascii_lowercase().as_str() {
        "artist" => Ok(TagField::Artist),
        "title" => Ok(TagField::Title),
        "album" => Ok(TagField::Album),
        "albumartist" => Ok(TagField::AlbumArtist),
        "track" => Ok(TagField::TrackNumber),
        "tracktotal" => Ok(TagField::TrackTotal),
        "disc" => Ok(TagField::DiscNumber),
        "year" => Ok(TagField::Year),
        "genre" => Ok(TagField::Genre),
        "comment" => Ok(TagField::Comment),
        "composer" => Ok(TagField::Composer),
        "publisher" => Ok(TagField::Publisher),
        "bpm" => Ok(TagField::Bpm),
        "isrc" => Ok(TagField::Isrc),
        "key" => Ok(TagField::InitialKey),
        "catalognumber" => Ok(TagField::CatalogNumber),
        _ => Err(MaskError::UnknownPlaceholder(name.to_string())),
    }
}

fn field_name(field: &TagField) -> &'static str {
    match field {
        TagField::Artist => "artist",
        TagField::Title => "title",
        TagField::Album => "album",
        TagField::AlbumArtist => "albumartist",
        TagField::TrackNumber => "track",
        TagField::TrackTotal => "tracktotal",
        TagField::DiscNumber => "disc",
        TagField::Year => "year",
        TagField::Genre => "genre",
        TagField::Comment => "comment",
        TagField::Composer => "composer",
        TagField::Publisher => "publisher",
        TagField::Bpm => "bpm",
        TagField::Isrc => "isrc",
        TagField::InitialKey => "key",
        TagField::CatalogNumber => "catalognumber",
        TagField::Custom(_) => "custom",
    }
}

/// Path separators in a tag value would otherwise split the rendered string
/// across directories, or fail outright on Windows. Other filesystem-
/// reserved characters (`:`, `*`, `?`, ...) are left alone -- that's the
/// future rename/apply step's job (architecture.md), not the mask grammar.
fn sanitize_for_filename(value: &str) -> Cow<'_, str> {
    if value.contains(['/', '\\']) {
        Cow::Owned(value.replace(['/', '\\'], "_"))
    } else {
        Cow::Borrowed(value)
    }
}

#[derive(Debug, Error)]
pub enum MaskError {
    #[error("unknown placeholder: %{0}%")]
    UnknownPlaceholder(String),
    #[error("ambiguous pattern: adjacent placeholders without a separator")]
    Ambiguous,
    #[error("missing tag for placeholder: %{0}%")]
    MissingTag(String),
    #[error("pattern does not match the filename")]
    NoMatch,
    #[error("unbalanced section brackets")]
    UnbalancedSection,
    #[error("unterminated quote: a ' must be closed by another '")]
    UnterminatedQuote,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(TagField, &str)]) -> TagMap {
        pairs
            .iter()
            .map(|(field, value)| (field.clone(), value.to_string()))
            .collect()
    }

    #[test]
    fn renders_tags_into_filename() {
        let mask = Mask::parse("%artist% - %title%").unwrap();
        let rendered = mask
            .render(&tags(&[
                (TagField::Artist, "Boards of Canada"),
                (TagField::Title, "Roygbiv"),
            ]))
            .unwrap();
        assert_eq!(rendered, "Boards of Canada - Roygbiv");
    }

    #[test]
    fn render_fails_on_missing_tag() {
        let mask = Mask::parse("%artist% - %title%").unwrap();
        let err = mask
            .render(&tags(&[(TagField::Artist, "Boards of Canada")]))
            .unwrap_err();
        assert!(matches!(err, MaskError::MissingTag(field) if field == "title"));
    }

    #[test]
    fn render_replaces_path_separators_in_values() {
        let mask = Mask::parse("%artist% - %title%").unwrap();
        let rendered = mask
            .render(&tags(&[
                (TagField::Artist, "AC/DC"),
                (TagField::Title, "T.N.T."),
            ]))
            .unwrap();
        assert_eq!(rendered, "AC_DC - T.N.T.");
    }

    #[test]
    fn extracts_tags_from_filename() {
        let mask = Mask::parse("%track% - %artist% - %title%").unwrap();
        let extracted = mask
            .extract("07 - Babes & Dudes - Why Tell Me Why")
            .unwrap();
        assert_eq!(extracted.get(&TagField::TrackNumber).unwrap(), "07");
        assert_eq!(extracted.get(&TagField::Artist).unwrap(), "Babes & Dudes");
        assert_eq!(extracted.get(&TagField::Title).unwrap(), "Why Tell Me Why");
    }

    #[test]
    fn extract_fails_when_filename_does_not_match() {
        let mask = Mask::parse("%artist% - %title%").unwrap();
        assert!(matches!(
            mask.extract("not the right shape at all"),
            Err(MaskError::NoMatch)
        ));
    }

    #[test]
    fn render_then_extract_round_trips() {
        let mask = Mask::parse("%artist% - %title% (%year%)").unwrap();
        let original = tags(&[
            (TagField::Artist, "Boards of Canada"),
            (TagField::Title, "Roygbiv"),
            (TagField::Year, "1998"),
        ]);

        let rendered = mask.render(&original).unwrap();
        let extracted = mask.extract(&rendered).unwrap();

        assert_eq!(extracted, original);
    }

    #[test]
    fn rejects_unknown_placeholder() {
        assert!(matches!(
            Mask::parse("%artist% - %bogus%"),
            Err(MaskError::UnknownPlaceholder(name)) if name == "bogus"
        ));
    }

    #[test]
    fn adjacent_placeholders_render_but_cannot_be_extracted() {
        // Rendering is unambiguous, so the pattern must parse and render.
        let mask = Mask::parse("%disc%%track%. %artist% - %title%").unwrap();
        let mut tags = TagMap::new();
        tags.insert(TagField::DiscNumber, "1".into());
        tags.insert(TagField::TrackNumber, "1".into());
        tags.insert(TagField::Artist, "The X Factor".into());
        tags.insert(TagField::Title, "Desert Rain".into());
        assert_eq!(
            mask.render(&tags).unwrap(),
            "101. The X Factor - Desert Rain"
        );

        // Splitting "101" back into disc and track is guesswork, so extraction
        // refuses instead of inventing a boundary.
        assert!(matches!(
            mask.extract("101. The X Factor - Desert Rain"),
            Err(MaskError::Ambiguous)
        ));
    }

    #[test]
    fn track_numbers_are_zero_padded_by_default() {
        let mask = Mask::parse("%track%. %title%").unwrap();
        let render = |track: &str| {
            let mut tags = TagMap::new();
            tags.insert(TagField::TrackNumber, track.into());
            tags.insert(TagField::Title, "Radio".into());
            mask.render(&tags).unwrap()
        };
        assert_eq!(render("1"), "01. Radio");
        assert_eq!(render("9"), "09. Radio");
        // Already wide enough -> untouched.
        assert_eq!(render("10"), "10. Radio");
        assert_eq!(render("123"), "123. Radio");
        // Non-numeric positions must not be mangled.
        assert_eq!(render("A1"), "A1. Radio");
    }

    #[test]
    fn conditional_section_is_dropped_whole_when_its_tags_are_absent() {
        let mask = Mask::parse("%album%[ (%year%)]").unwrap();
        let mut tags = TagMap::new();
        tags.insert(TagField::Album, "La Bush".into());

        // No year: the section goes, and takes its leading space with it.
        assert_eq!(mask.render(&tags).unwrap(), "La Bush");

        tags.insert(TagField::Year, "1996".into());
        assert_eq!(mask.render(&tags).unwrap(), "La Bush (1996)");

        // A present-but-empty tag counts as absent for the section.
        tags.insert(TagField::Year, String::new());
        assert_eq!(mask.render(&tags).unwrap(), "La Bush");
    }

    #[test]
    fn a_missing_tag_outside_a_section_is_still_an_error() {
        // Optionality has to be asked for; an unsatisfiable mask must not
        // quietly render a half-built name.
        let mask = Mask::parse("%artist% - %title%").unwrap();
        let mut tags = TagMap::new();
        tags.insert(TagField::Artist, "Plastic".into());
        assert!(matches!(
            mask.render(&tags),
            Err(MaskError::MissingTag(field)) if field == "title"
        ));
    }

    #[test]
    fn sections_nest_and_a_filled_inner_section_keeps_the_outer() {
        let mask = Mask::parse("%album%[ (%year%[, %genre%])]").unwrap();
        let mut tags = TagMap::new();
        tags.insert(TagField::Album, "La Bush".into());
        assert_eq!(mask.render(&tags).unwrap(), "La Bush");

        tags.insert(TagField::Year, "1996".into());
        assert_eq!(mask.render(&tags).unwrap(), "La Bush (1996)");

        tags.insert(TagField::Genre, "Trance".into());
        assert_eq!(mask.render(&tags).unwrap(), "La Bush (1996, Trance)");
    }

    #[test]
    fn renders_a_real_multi_disc_pattern() {
        // Straight from a working configuration: several optional parts, and
        // `%disc%%track%` with no separator between them.
        let mask =
            Mask::parse("[%albumartist%] - %album%[ (%year%)]/%disc%%track% [%artist% - ]%title%")
                .unwrap();

        let mut tags = TagMap::new();
        tags.insert(TagField::AlbumArtist, "Various".into());
        tags.insert(TagField::Album, "La Bush".into());
        tags.insert(TagField::Year, "1996".into());
        tags.insert(TagField::DiscNumber, "1".into());
        tags.insert(TagField::TrackNumber, "1".into());
        tags.insert(TagField::Artist, "The X Factor".into());
        tags.insert(TagField::Title, "Desert Rain".into());
        assert_eq!(
            mask.render(&tags).unwrap(),
            "Various - La Bush (1996)/101 The X Factor - Desert Rain"
        );

        // A single-artist album with no year: both optional parts vanish
        // cleanly, leaving no stray separators.
        let mut sparse = TagMap::new();
        sparse.insert(TagField::AlbumArtist, "Boards Of Canada".into());
        sparse.insert(TagField::Album, "Geogaddi".into());
        sparse.insert(TagField::DiscNumber, "1".into());
        sparse.insert(TagField::TrackNumber, "4".into());
        sparse.insert(TagField::Title, "Sunshine Recorder".into());
        assert_eq!(
            mask.render(&sparse).unwrap(),
            "Boards Of Canada - Geogaddi/104 Sunshine Recorder"
        );
    }

    #[test]
    fn quotes_escape_reserved_characters() {
        let mask = Mask::parse("'['%artist%']' - %title%").unwrap();
        let mut tags = TagMap::new();
        tags.insert(TagField::Artist, "Plastic".into());
        tags.insert(TagField::Title, "Sexy Groove".into());
        // The brackets are literal here, not a conditional section.
        assert_eq!(mask.render(&tags).unwrap(), "[Plastic] - Sexy Groove");

        // A doubled quote is one literal quote.
        let quoted = Mask::parse("%artist%'' - %title%").unwrap();
        assert_eq!(quoted.render(&tags).unwrap(), "Plastic' - Sexy Groove");
    }

    #[test]
    fn unbalanced_sections_and_quotes_are_rejected() {
        assert!(matches!(
            Mask::parse("%album%[ (%year%)"),
            Err(MaskError::UnbalancedSection)
        ));
        assert!(matches!(
            Mask::parse("%album%] "),
            Err(MaskError::UnbalancedSection)
        ));
        assert!(matches!(
            Mask::parse("'[%artist%"),
            Err(MaskError::UnterminatedQuote)
        ));
    }

    #[test]
    fn an_optional_part_extracts_when_present_and_is_skipped_when_not() {
        let mask = Mask::parse("%artist% - %title%[ (%year%)]").unwrap();

        let with_year = mask.extract("Plastic - Sexy Groove (1996)").unwrap();
        assert_eq!(
            with_year.get(&TagField::Year).map(String::as_str),
            Some("1996")
        );
        assert_eq!(
            with_year.get(&TagField::Title).map(String::as_str),
            Some("Sexy Groove")
        );

        let without = mask.extract("Plastic - Sexy Groove").unwrap();
        assert_eq!(without.get(&TagField::Year), None);
        assert_eq!(
            without.get(&TagField::Title).map(String::as_str),
            Some("Sexy Groove")
        );
    }

    #[test]
    fn placeholder_width_can_be_set_explicitly() {
        let mut tags = TagMap::new();
        tags.insert(TagField::DiscNumber, "2".into());
        tags.insert(TagField::TrackNumber, "7".into());

        // Widen the disc for a large box set...
        let wide = Mask::parse("%disc:2%%track%").unwrap();
        assert_eq!(wide.render(&tags).unwrap(), "0207");

        // ...or opt out of the default track padding.
        let plain = Mask::parse("%disc%-%track:1%").unwrap();
        assert_eq!(plain.render(&tags).unwrap(), "2-7");

        // A malformed width is a bad placeholder, not a silent default.
        assert!(matches!(
            Mask::parse("%track:x%"),
            Err(MaskError::UnknownPlaceholder(_))
        ));
    }

    #[test]
    fn rejects_unterminated_placeholder() {
        assert!(matches!(
            Mask::parse("%artist% - %title"),
            Err(MaskError::UnknownPlaceholder(_))
        ));
    }

    #[test]
    fn literal_only_pattern_has_no_placeholders() {
        let mask = Mask::parse("static-name").unwrap();
        assert_eq!(mask.render(&TagMap::new()).unwrap(), "static-name");
        assert!(mask.extract("static-name").is_ok());
        assert!(mask.extract("other-name").is_err());
    }
}
