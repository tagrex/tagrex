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
//!   numbers pad to two digits by default). A width *written out* is also a
//!   fixed length on extract (#140), which is what lets `%disc:1%%track:2%`
//!   split `101` where two open-ended placeholders can't.
//! - `[...]` — a conditional section, kept only when a placeholder inside it
//!   resolved to something. This is what lets one mask serve a library where
//!   some albums have a year and some don't, without emitting stray separators.
//! - `'x'` — a literal, for the reserved characters `% [ ]`; `''` is one quote.
//! - `%skip%` — a discard placeholder (#70): on *extract* it matches and throws
//!   away a run of text (filename junk that maps to no tag), and may repeat; it
//!   has no render value, so a mask carrying it is extract-only. It's the mirror
//!   of `%side%`, which is render-only.
//! - File and technical placeholders (#147) — `%filename%`, `%foldername%`,
//!   `%_bitrate%`, … — properties of the *file* rather than of its tags. They
//!   read from a [`FileContext`] instead of the [`TagMap`], and they are all
//!   render-only for the same reason `%side%` is: there is no tag to extract a
//!   bitrate into, and pulling `%filename%` back out of a filename is a
//!   tautology.
//! - `$name(arg,arg)` — a function call (#73), which is what turns the pattern
//!   from a substitution into a small expression language. Arguments are
//!   themselves patterns, so they nest and may hold placeholders and sections;
//!   `,` and `)` end an argument, and `','` writes a literal comma. The library
//!   itself lives in [`functions`].
//!
//! **A mask that calls a function is render-only.** Substitution is invertible
//! and that is the whole basis of the two directions; `$upper` is not, and
//! guessing which half of `THE BEATLES` was upper-cased by the pattern and which
//! by the file is exactly the invention this module refuses to make elsewhere.
//! A `$` that is not followed by a name and a `(` stays an ordinary character,
//! so patterns written before functions existed keep working.
//!
//! Beyond those, only the first-class [`TagField`] variants are valid
//! placeholder names — `Custom` fields aren't addressable from a mask yet.
//! Deferred rather than ignored, same as scripting in architecture.md.

mod functions;

use std::borrow::Cow;
use std::path::Path;
use std::time::SystemTime;

use regex::Regex;
use thiserror::Error;

use crate::model::{AudioFormat, AudioProps, TagField, TagMap};
use functions::{function_from_name, Function, ALL_FUNCTIONS};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(String),
    /// A field, the minimum width it renders to (zero-padded; `1` means "print
    /// it as-is"), and whether that width was **stated in the pattern**.
    ///
    /// Only a stated width is a fixed length on extract (#140). The width the
    /// parser fills in by itself — two for track numbers — is about padding;
    /// treating it as a match length would break every `%track%` pattern
    /// against a name that holds a plain `5`.
    Placeholder(TagField, usize, bool),
    /// A conditional section, `[...]`. Rendered only when at least one
    /// placeholder inside it resolves to a non-empty value, and dropped whole
    /// otherwise — which is what lets ` [%artist% - ]` contribute nothing (not
    /// even its space) on a single-artist album.
    Section(Vec<Segment>),
    /// `%side%` — the vinyl/cassette side as a letter (disc 1 -> A, 2 -> B, …),
    /// or empty for other media (#106). A computed presentation value, not a
    /// field, so it renders but a mask containing it can't extract.
    Side,
    /// `%skip%` — a discard placeholder (#70): on extract it matches a run of
    /// text and throws it away (filenames are full of junk that maps to no tag);
    /// it may repeat, each occurrence independent. It's the inverse of `%side%`
    /// — meaningless to render, so a mask carrying it is extract-only.
    Skip,
    /// A property of the file rather than one of its tags (#147). Render-only,
    /// like [`Side`](Self::Side).
    File(FileValue),
    /// `$name(arg,arg)` — a function over its arguments (#73). Each argument is
    /// a segment list of its own, which is what lets calls nest and lets a
    /// placeholder or a `[…]` section sit inside one. Render-only: a function
    /// transforms, and a transformation cannot be run backwards out of a
    /// filename.
    Call(Function, Vec<Vec<Segment>>),
}

/// A property of the file itself, addressable from a mask (#147).
///
/// Two families, distinguished by their spelling so a reader can tell at a
/// glance which is which: the **path** ones (`%filename%`, `%foldername%`, …)
/// name where the file lives, and the **technical** ones (`%_bitrate%`,
/// `%_length%`, …) describe the audio and carry a leading underscore. All of
/// them are render-only — a bitrate is not something a filename can be parsed
/// back into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileValue {
    /// `%filename%` — the name without its extension.
    Name,
    /// `%fileext%` — the extension alone, without the dot.
    Ext,
    /// `%filenameext%` — name and extension together.
    NameExt,
    /// `%filepath%` — the full path. Sanitized like every other rendered value,
    /// so its separators are stripped: an identifier, not a reconstructable path.
    Path,
    /// `%foldername%` / `%foldername2%` / `%foldername3%` — the containing
    /// folder and its ancestors, `1` being the immediate parent.
    Folder(usize),
    /// `%_length%` — playback duration as `m:ss` (`h:mm:ss` past an hour).
    Length,
    /// `%_length_sec%` — playback duration in whole seconds.
    LengthSec,
    /// `%_bitrate%` — audio bitrate in kbps.
    Bitrate,
    /// `%_samplerate%` — sample rate in Hz.
    SampleRate,
    /// `%_channels%` — channel count.
    Channels,
    /// `%_codec%` — the container's common name (`MP3`, `FLAC`, `APE`).
    Codec,
    /// `%_filesize%` — file size, human-readable (`7.3 MB`).
    FileSize,
    /// `%_filesize_bytes%` — file size in bytes.
    FileSizeBytes,
    /// `%_filedate%` — last-modified date as `YYYY-MM-DD`, UTC.
    FileDate,
}

impl FileValue {
    /// Whether reading this needs the audio properties, which cost a probe of
    /// the file. Callers use [`Mask::needs_audio_props`] to skip that read for
    /// the vast majority of masks, which ask for none of these.
    fn needs_audio_props(self) -> bool {
        matches!(
            self,
            Self::Length | Self::LengthSec | Self::Bitrate | Self::SampleRate | Self::Channels
        )
    }

    /// Whether reading this needs the filesystem metadata (size, mtime).
    fn needs_metadata(self) -> bool {
        matches!(self, Self::FileSize | Self::FileSizeBytes | Self::FileDate)
    }
}

/// The file a mask is rendering *about* (#147) — everything the file
/// placeholders read, none of which lives in a [`TagMap`].
///
/// Every part is optional and an absent one renders as empty, exactly like a
/// `%side%` on a CD: a mask must never fail because the caller had no bitrate to
/// hand. Assemble it with the [`Mask::needs_audio_props`] /
/// [`Mask::needs_metadata`] guards so a pattern that asks for none of this
/// costs no extra reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileContext<'a> {
    pub path: Option<&'a Path>,
    pub format: Option<AudioFormat>,
    pub props: Option<AudioProps>,
    pub size_bytes: Option<u64>,
    pub modified: Option<SystemTime>,
}

impl<'a> FileContext<'a> {
    /// Gather what `mask` asks about `track` — the one place a mask causes file
    /// I/O, deliberately here at the boundary rather than inside `render`, which
    /// stays a pure string operation.
    ///
    /// The path and container are free (the caller already has the `TrackFile`);
    /// a probe and a `metadata` call happen only when the pattern actually reads
    /// an audio property or a size/date, so the overwhelmingly common
    /// tags-only mask costs nothing extra. Anything unreadable stays `None` and
    /// renders empty — a mask must not fail over a file property.
    pub fn read(mask: &Mask, track: &'a crate::model::TrackFile) -> Self {
        let metadata = mask
            .needs_metadata()
            .then(|| std::fs::metadata(&track.path).ok())
            .flatten();
        Self {
            path: Some(&track.path),
            format: Some(track.format),
            props: mask
                .needs_audio_props()
                .then(|| crate::model::TagEngine::read_audio_props(&track.path).ok())
                .flatten(),
            size_bytes: metadata.as_ref().map(|m| m.len()),
            modified: metadata.as_ref().and_then(|m| m.modified().ok()),
        }
    }
}

/// A parsed, validated mask pattern.
#[derive(Debug, Clone)]
pub struct Mask {
    pattern: String,
    segments: Vec<Segment>,
    regex: Regex,
    /// Two placeholders with nothing between them and no stated width to split
    /// on. Rendering them is perfectly well-defined (`%disc%%track%` -> `101`);
    /// *extracting* them is not, since nothing says where one value ends and
    /// the next begins. So this is only an error for the extract direction, not
    /// for the pattern as such — and it goes away as soon as one of the pair
    /// states its width (`%disc:1%%track:2%`, #140).
    adjacent_placeholders: bool,
    /// The pattern contains a `%side%` or a file placeholder (#147) — a computed
    /// or file-derived value. Those render fine, but there's no tag to extract
    /// them back into, so the mask is render-only (the extract direction
    /// refuses it).
    render_only: bool,
    /// The pattern reads an audio property, so rendering it needs a probe of the
    /// file. Precomputed here so a caller can skip that read per file (#147).
    needs_audio_props: bool,
    /// The pattern reads file size or mtime, so rendering it needs a `metadata`
    /// call. Precomputed for the same reason (#147).
    needs_metadata: bool,
    /// The pattern contains a `%skip%`, a discard placeholder (#70). It extracts
    /// fine (matching and throwing away a run of text), but there's nothing to
    /// render for it, so the mask is extract-only (the render direction refuses it).
    extract_only: bool,
}

impl Mask {
    /// Parse and validate a pattern string.
    pub fn parse(pattern: &str) -> Result<Self, MaskError> {
        let segments = parse_segments(pattern)?;
        let mut previous = None;
        let adjacent_placeholders = has_ambiguous_adjacency(&segments, &mut previous);
        let render_only =
            has_side(&segments) || has_file(&segments, |_| true) || has_call(&segments);
        let extract_only = has_skip(&segments);
        let needs_audio_props = has_file(&segments, FileValue::needs_audio_props);
        let needs_metadata = has_file(&segments, FileValue::needs_metadata);
        let regex = build_regex(&segments);
        Ok(Self {
            pattern: pattern.to_string(),
            segments,
            regex,
            adjacent_placeholders,
            render_only,
            extract_only,
            needs_audio_props,
            needs_metadata,
        })
    }

    /// Tags -> filename (the Music Renamer direction), with no file to read
    /// properties from: the file placeholders (#147) render empty, the way
    /// `%side%` does on a CD. Use [`render_with`](Self::render_with) wherever
    /// the file is known.
    pub fn render(&self, tags: &TagMap) -> Result<String, MaskError> {
        self.render_with(tags, &FileContext::default())
    }

    /// Tags + the file itself -> filename (#147).
    pub fn render_with(&self, tags: &TagMap, file: &FileContext<'_>) -> Result<String, MaskError> {
        // `%skip%` discards text on extract and has nothing to render (#70).
        if self.extract_only {
            return Err(MaskError::ExtractOnly);
        }
        let mut out = String::new();
        render_segments(&self.segments, tags, file, false, &mut out)?;
        Ok(out)
    }

    /// Whether rendering this pattern needs [`FileContext::props`], which costs
    /// a probe of the file. False for every mask that asks for no audio
    /// property — which is nearly all of them, so this is worth checking before
    /// reading one per file (#147).
    pub fn needs_audio_props(&self) -> bool {
        self.needs_audio_props
    }

    /// Whether rendering this pattern needs [`FileContext::size_bytes`] or
    /// [`FileContext::modified`], i.e. a filesystem `metadata` call (#147).
    pub fn needs_metadata(&self) -> bool {
        self.needs_metadata
    }

    /// Filename -> tags (the import direction).
    pub fn extract(&self, filename: &str) -> Result<TagMap, MaskError> {
        // Rendering adjacent placeholders is fine; splitting the result back
        // apart is guesswork, so refuse rather than invent a boundary.
        if self.adjacent_placeholders {
            return Err(MaskError::Ambiguous);
        }
        // `%side%` and the file placeholders (#147) are computed or file-derived
        // values with no tag to extract into.
        if self.render_only {
            return Err(MaskError::RenderOnly);
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
    let (segments, _) = parse_until(&chars, &mut position, &[])?;
    debug_assert_eq!(position, chars.len());
    Ok(segments)
}

/// Parse segments until one of `terminators` (or end of input), returning which
/// one stopped it — `None` for end of input.
///
/// Recursive because sections and calls nest. Handing the terminator back rather
/// than erroring on end-of-input is what lets one loop serve three callers that
/// disagree about whether running out is a problem: at the top level it is the
/// normal ending, inside `[…]` it is an unbalanced bracket, and inside a call it
/// is a missing `)`. Each caller says so in its own words.
fn parse_until(
    chars: &[char],
    position: &mut usize,
    terminators: &[char],
) -> Result<(Vec<Segment>, Option<char>), MaskError> {
    let mut segments = Vec::new();
    let mut literal = String::new();

    while *position < chars.len() {
        let current = chars[*position];
        if terminators.contains(&current) {
            *position += 1;
            flush_literal(&mut literal, &mut segments);
            return Ok((segments, Some(current)));
        }
        match current {
            '[' => {
                *position += 1;
                flush_literal(&mut literal, &mut segments);
                let (inner, closed) = parse_until(chars, position, &[']'])?;
                if closed.is_none() {
                    return Err(MaskError::UnbalancedSection);
                }
                segments.push(Segment::Section(inner));
            }
            // A call, but only when a name and a `(` really follow: a lone `$`
            // is an ordinary character, so a pattern written before functions
            // existed still means what it meant (#73).
            '$' => match parse_call(chars, position)? {
                Some(call) => {
                    flush_literal(&mut literal, &mut segments);
                    segments.push(call);
                }
                None => {
                    literal.push('$');
                    *position += 1;
                }
            },
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

    flush_literal(&mut literal, &mut segments);
    Ok((segments, None))
}

/// Parse `$name(arg,arg)` starting at the `$`, or `None` when this `$` doesn't
/// begin a call at all (#73).
///
/// The lookahead is what makes the sigil safe to introduce into a grammar that
/// already ships: only `$` + a name + `(` is a call, so a pattern containing a
/// price or a Windows variable keeps rendering as text. A name that *is*
/// followed by `(` but isn't a function is an error rather than literal text —
/// `$upprer(%title%)` is a typo, and silently writing it into a filename would
/// be the worse answer.
fn parse_call(chars: &[char], position: &mut usize) -> Result<Option<Segment>, MaskError> {
    let mut cursor = *position + 1;
    while cursor < chars.len() && (chars[cursor].is_ascii_alphanumeric() || chars[cursor] == '_') {
        cursor += 1;
    }
    let name: String = chars[*position + 1..cursor].iter().collect();
    if name.is_empty() || chars.get(cursor) != Some(&'(') {
        return Ok(None);
    }
    let function = function_from_name(&name).ok_or(MaskError::UnknownFunction(name.clone()))?;

    *position = cursor + 1;
    let mut arguments = Vec::new();
    loop {
        let (argument, closed) = parse_until(chars, position, &[',', ')'])?;
        match closed {
            Some(')') => {
                arguments.push(argument);
                break;
            }
            Some(_) => arguments.push(argument),
            None => return Err(MaskError::UnclosedCall(name)),
        }
    }

    // Counted here rather than at render time: a miscounted call is a mistake in
    // the pattern, and the pattern is being typed with a live preview next to
    // it. `$upper()` reads as one empty argument, which is why no function needs
    // an arity of zero for that to be well-defined.
    let (minimum, maximum) = function.arity();
    if arguments.len() < minimum || maximum.is_some_and(|most| arguments.len() > most) {
        return Err(MaskError::BadArity {
            name: function.name(),
            expected: match maximum {
                Some(most) if most == minimum => format!("{minimum}"),
                Some(most) => format!("{minimum} to {most}"),
                None => format!("{minimum} or more"),
            },
            actual: arguments.len(),
        });
    }
    Ok(Some(Segment::Call(function, arguments)))
}

fn flush_literal(literal: &mut String, segments: &mut Vec<Segment>) {
    if !literal.is_empty() {
        segments.push(Segment::Literal(std::mem::take(literal)));
    }
}

/// `name` or `name:width`, e.g. `%track:3%`.
fn parse_placeholder(spec: &str) -> Result<Segment, MaskError> {
    if spec.eq_ignore_ascii_case("side") {
        return Ok(Segment::Side);
    }
    if spec.eq_ignore_ascii_case("skip") {
        return Ok(Segment::Skip);
    }
    // Matched on the whole spec, before the `:width` split: a width is about
    // zero-padding a number, which none of these are, so `%filename:5%` is an
    // unknown placeholder rather than a silently ignored width (#147).
    if let Some(value) = file_value_from_name(spec) {
        return Ok(Segment::File(value));
    }
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
    // `%track:0%` states nothing useful, so it counts as unstated and falls back
    // to the default like a bare `%track%`.
    let stated = width > 0;
    let width = if stated { width } else { default_width(&field) };
    Ok(Segment::Placeholder(field, width, stated))
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
            // A stated width is a fixed length (#140): it's what lets
            // `%disc:1%%track:2%` split `101` where two open-ended captures
            // couldn't. For the integer fields the run has to be digits, so a
            // name that doesn't actually carry a number there fails to match —
            // and its file is skipped — instead of capturing two letters as a
            // track number for the writer to reject later.
            Segment::Placeholder(field, width, true) => {
                let run = if is_integer_field(field) { r"\d" } else { "." };
                out.push_str(&format!("(?P<{}>{run}{{{width}}})", group_name(*index)));
                *index += 1;
            }
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
            // `%side%` is render-only (a mask carrying it refuses to extract), so
            // it contributes no capture group and no index.
            Segment::Side => {}
            // `%skip%` matches a run of text but keeps none of it: a non-capturing
            // group, so it consumes no index in either this walk or collect_captures.
            Segment::Skip => out.push_str("(?:.+?)"),
            // Render-only like `%side%`: a mask carrying one refuses to extract,
            // so it contributes no capture group and no index (#147).
            Segment::File(_) => {}
            // Render-only too (#73), and for a stronger reason: a function is
            // not invertible, so there is nothing to capture.
            Segment::Call(..) => {}
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
            Segment::Placeholder(field, ..) => {
                if let Some(matched) = captures.name(&group_name(*index)) {
                    tags.insert(field.clone(), matched.as_str().to_string());
                }
                *index += 1;
            }
            Segment::Section(inner) => collect_captures(inner, captures, index, tags),
            // Render-only; extraction is refused before reaching here.
            Segment::Side => {}
            // Matched a run of text but writes no tag (a non-capturing group, #70).
            Segment::Skip => {}
            // Render-only; extraction is refused before reaching here (#147).
            Segment::File(_) => {}
            // Render-only; extraction is refused before reaching here (#73).
            Segment::Call(..) => {}
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
    file: &FileContext<'_>,
    optional: bool,
    out: &mut String,
) -> Result<bool, MaskError> {
    let mut produced = false;
    for segment in segments {
        match segment {
            Segment::Literal(text) => out.push_str(text),
            Segment::Placeholder(field, width, _) => match tags.get(field) {
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
                if render_segments(inner, tags, file, true, &mut buffer)? {
                    out.push_str(&buffer);
                    produced = true;
                }
            }
            // `%side%` renders the disc as a side letter on side-based media, and
            // nothing otherwise -- never an error (empty is a valid outcome).
            Segment::Side => {
                if let Some(letter) = side_letter_for(tags) {
                    out.push(letter);
                    produced = true;
                }
            }
            // `%skip%` has no render value; render() refuses the mask before we
            // get here, so this is just defensive exhaustiveness (#70).
            Segment::Skip => return Err(MaskError::ExtractOnly),
            // A file property (#147). Sanitized like a tag value, since it lands
            // in the same filename; empty whenever the caller had nothing to give
            // (no path, no probe) — never an error, same as `%side%` on a CD.
            Segment::File(value) => {
                let rendered = render_file_value(*value, file);
                let clean = sanitize_for_filename(&rendered);
                if !clean.is_empty() {
                    produced = true;
                }
                out.push_str(&clean);
            }
            // A function call (#73). Each argument is a pattern in its own
            // right, rendered into a string first — which is what makes the
            // calls nest.
            //
            // `optional` is passed straight through rather than forced on: a
            // bare `%artist%` on a file with no artist is an unsatisfiable
            // pattern, and wrapping it in `$upper()` must not quietly turn that
            // into an empty string. Inside a `[…]` it stays as forgiving as
            // everything else there.
            //
            // The values arrive already sanitized, because the placeholders
            // inside the arguments sanitized them; the result is not sanitized
            // again, so a separator a *pattern* puts there deliberately still
            // means what it does everywhere else in the mask.
            Segment::Call(function, arguments) => {
                let mut values = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    let mut buffer = String::new();
                    render_segments(argument, tags, file, optional, &mut buffer)?;
                    values.push(buffer);
                }
                let value = function.apply(&values)?;
                if !value.is_empty() {
                    produced = true;
                }
                out.push_str(&value);
            }
        }
    }
    Ok(produced)
}

/// One file placeholder's value, or empty when the context doesn't carry it.
fn render_file_value(value: FileValue, file: &FileContext<'_>) -> String {
    let component = |part: Option<&std::ffi::OsStr>| {
        part.and_then(|p| p.to_str())
            .map(str::to_string)
            .unwrap_or_default()
    };
    match value {
        FileValue::Name => file
            .path
            .map(|p| component(p.file_stem()))
            .unwrap_or_default(),
        FileValue::Ext => file
            .path
            .map(|p| component(p.extension()))
            .unwrap_or_default(),
        FileValue::NameExt => file
            .path
            .map(|p| component(p.file_name()))
            .unwrap_or_default(),
        FileValue::Path => file
            .path
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        // `1` is the immediate parent, `2` its parent, and so on. Walking up
        // past the root simply yields nothing rather than an error — a mask
        // asking for a third folder level on a file two deep is a thin result,
        // not a broken pattern.
        FileValue::Folder(level) => file
            .path
            .and_then(|p| p.ancestors().nth(level))
            .map(|p| component(p.file_name()))
            .unwrap_or_default(),
        FileValue::Length => file
            .props
            .map(|p| format_duration(p.duration_secs))
            .unwrap_or_default(),
        FileValue::LengthSec => file
            .props
            .map(|p| p.duration_secs.to_string())
            .unwrap_or_default(),
        FileValue::Bitrate => optional_number(file.props.and_then(|p| p.bitrate_kbps)),
        FileValue::SampleRate => optional_number(file.props.and_then(|p| p.sample_rate_hz)),
        FileValue::Channels => optional_number(file.props.and_then(|p| p.channels)),
        FileValue::Codec => file
            .format
            .map(|f| f.name().to_string())
            .unwrap_or_default(),
        FileValue::FileSize => file.size_bytes.map(format_size).unwrap_or_default(),
        FileValue::FileSizeBytes => optional_number(file.size_bytes),
        FileValue::FileDate => file.modified.and_then(format_date).unwrap_or_default(),
    }
}

fn optional_number(value: Option<impl std::fmt::Display>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

/// `m:ss`, or `h:mm:ss` once there's an hour to show. Seconds are always two
/// digits so a column of durations lines up.
fn format_duration(seconds: u64) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// A human-readable size, binary units (what a file manager shows), one decimal
/// above the KB threshold. Bytes are printed whole — `512 B`, not `0.5 KB`.
fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// `YYYY-MM-DD` in UTC. A local-time rendering would make the same file produce
/// different names on two machines, which is precisely what a rename mask must
/// not do; UTC keeps it reproducible.
fn format_date(time: SystemTime) -> Option<String> {
    let seconds = time.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
    let (year, month, day) = civil_from_days((seconds / 86_400) as i64);
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// Days since the Unix epoch -> calendar date (Howard Hinnant's `civil_from_days`,
/// the standard branch-free conversion). Pulled in rather than a date crate: one
/// arithmetic function is a smaller dependency surface than a calendar library
/// we'd use for exactly this.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (year + i64::from(month <= 2), month, day)
}

/// The side letter for `%side%`: only on side-based media (vinyl, cassette), and
/// only when the disc number is 1..=26 -> 'A'..='Z'. Empty (None) otherwise.
fn side_letter_for(tags: &TagMap) -> Option<char> {
    let media = tags.get(&TagField::MediaType)?;
    if !is_side_medium(media) {
        return None;
    }
    let disc: u32 = tags.get(&TagField::DiscNumber)?.trim().parse().ok()?;
    (1..=26)
        .contains(&disc)
        .then(|| (b'A' + (disc as u8 - 1)) as char)
}

/// Whether a media-type value denotes a side-based medium (vinyl / cassette),
/// whose track positions are conventionally written as a side letter + number.
pub(crate) fn is_side_medium(media: &str) -> bool {
    let m = media.to_ascii_lowercase();
    [
        "vinyl", "lp", "shellac", "cassette", "tape", "\"", "acetate",
    ]
    .iter()
    .any(|needle| m.contains(needle))
}

/// Whether any segment is a `%side%` (makes the mask render-only).
fn has_side(segments: &[Segment]) -> bool {
    segments.iter().any(|segment| match segment {
        Segment::Side => true,
        Segment::Section(inner) => has_side(inner),
        Segment::Call(_, arguments) => arguments.iter().any(|a| has_side(a)),
        _ => false,
    })
}

/// Whether any segment is a `%skip%` (makes the mask extract-only, #70).
fn has_skip(segments: &[Segment]) -> bool {
    segments.iter().any(|segment| match segment {
        Segment::Skip => true,
        Segment::Section(inner) => has_skip(inner),
        Segment::Call(_, arguments) => arguments.iter().any(|a| has_skip(a)),
        _ => false,
    })
}

/// Whether any segment is a file placeholder `predicate` accepts (#147) — one
/// walk serving three questions: is the mask render-only, does it need a probe,
/// does it need filesystem metadata.
fn has_file(segments: &[Segment], predicate: fn(FileValue) -> bool) -> bool {
    segments.iter().any(|segment| match segment {
        Segment::File(value) => predicate(*value),
        Segment::Section(inner) => has_file(inner, predicate),
        // Arguments count: `$upper(%_codec%)` reads the container just as much
        // as a bare `%_codec%` does, and missing that would leave the render
        // without the probe it needs (#73).
        Segment::Call(_, arguments) => arguments.iter().any(|a| has_file(a, predicate)),
        _ => false,
    })
}

/// Whether any segment is a function call, which makes the mask render-only
/// (#73).
fn has_call(segments: &[Segment]) -> bool {
    segments.iter().any(|segment| match segment {
        Segment::Call(..) => true,
        Segment::Section(inner) => has_call(inner),
        _ => false,
    })
}

/// Two placeholders with no literal text between them *and* no width to split
/// them on, looking through section boundaries — `[%disc%]%track%` is just as
/// unsplittable as `%disc%%track%`.
///
/// A stated width is a boundary (#140), so a pair is only ambiguous when
/// neither side states one: `%disc:1%%track:2%` says exactly where `101`
/// divides, while `%disc%%track%` still doesn't. `previous` carries the
/// preceding segment: `None` when it wasn't a placeholder, otherwise whether
/// that placeholder stated a width.
fn has_ambiguous_adjacency(segments: &[Segment], previous: &mut Option<bool>) -> bool {
    for segment in segments {
        // `%skip%` and `%side%` count as placeholders here (`%skip%%title%` is
        // just as unsplittable as two fields with nothing between them, #70),
        // and neither can state a width.
        let stated = match segment {
            Segment::Placeholder(_, _, stated) => Some(*stated),
            Segment::Side | Segment::Skip | Segment::File(_) | Segment::Call(..) => Some(false),
            _ => None,
        };
        if let Some(stated) = stated {
            if *previous == Some(false) && !stated {
                return true;
            }
            *previous = Some(stated);
            continue;
        }
        match segment {
            Segment::Literal(text) => {
                if !text.is_empty() {
                    *previous = None;
                }
            }
            Segment::Section(inner) => {
                if has_ambiguous_adjacency(inner, previous) {
                    return true;
                }
            }
            _ => unreachable!("placeholder-like segments are handled above"),
        }
    }
    false
}

/// Whether the tag backend stores this field as a number, which is what makes a
/// digits-only run the right fixed-width match for it (#140). Same set the
/// writer's per-field validation constrains.
fn is_integer_field(field: &TagField) -> bool {
    matches!(
        field,
        TagField::TrackNumber | TagField::TrackTotal | TagField::DiscNumber | TagField::DiscTotal
    )
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

/// A file placeholder by name (#147), or `None` if the name isn't one — the
/// caller then tries the tag fields, so an unknown name still reports
/// `UnknownPlaceholder` naming the whole spec.
fn file_value_from_name(name: &str) -> Option<FileValue> {
    match name.to_ascii_lowercase().as_str() {
        "filename" => Some(FileValue::Name),
        "fileext" => Some(FileValue::Ext),
        "filenameext" => Some(FileValue::NameExt),
        "filepath" => Some(FileValue::Path),
        "foldername" => Some(FileValue::Folder(1)),
        "foldername2" => Some(FileValue::Folder(2)),
        "foldername3" => Some(FileValue::Folder(3)),
        "_length" => Some(FileValue::Length),
        "_length_sec" => Some(FileValue::LengthSec),
        "_bitrate" => Some(FileValue::Bitrate),
        "_samplerate" => Some(FileValue::SampleRate),
        "_channels" => Some(FileValue::Channels),
        "_codec" => Some(FileValue::Codec),
        "_filesize" => Some(FileValue::FileSize),
        "_filesize_bytes" => Some(FileValue::FileSizeBytes),
        "_filedate" => Some(FileValue::FileDate),
        _ => None,
    }
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
        "disctotal" => Ok(TagField::DiscTotal),
        "year" => Ok(TagField::Year),
        "genre" => Ok(TagField::Genre),
        "comment" => Ok(TagField::Comment),
        "composer" => Ok(TagField::Composer),
        "publisher" => Ok(TagField::Publisher),
        "bpm" => Ok(TagField::Bpm),
        "isrc" => Ok(TagField::Isrc),
        "key" => Ok(TagField::InitialKey),
        "catalognumber" => Ok(TagField::CatalogNumber),
        "url" => Ok(TagField::Url),
        "media" => Ok(TagField::MediaType),
        _ => Err(MaskError::UnknownPlaceholder(name.to_string())),
    }
}

fn field_name(field: &TagField) -> &'static str {
    tag_placeholder_doc(field).0
}

/// What a tag field is called in a mask, and how the reference describes it
/// (#148).
///
/// Exhaustive over [`TagField`] on purpose: adding a field breaks this match,
/// so a new field cannot reach the parser without someone deciding what to call
/// it *and* what the in-app reference says about it. That is the whole fix for
/// the problem this documents — a user who cannot see the list has no way to
/// guess that the catalogue number is `%catalognumber%` and not `%catno%`.
fn tag_placeholder_doc(field: &TagField) -> (&'static str, &'static str) {
    match field {
        TagField::Artist => ("artist", "Track artist"),
        TagField::Title => ("title", "Track title"),
        TagField::Album => ("album", "Album title"),
        TagField::AlbumArtist => ("albumartist", "Album artist — the release's credit"),
        TagField::TrackNumber => ("track", "Track number (pads to two digits)"),
        TagField::TrackTotal => ("tracktotal", "Number of tracks on the release"),
        TagField::DiscNumber => ("disc", "Disc number"),
        TagField::DiscTotal => ("disctotal", "Number of discs in the set"),
        TagField::Year => ("year", "Release year"),
        TagField::Genre => ("genre", "Genre"),
        TagField::Comment => ("comment", "Comment"),
        TagField::Composer => ("composer", "Composer"),
        TagField::Publisher => ("publisher", "Label / publisher"),
        TagField::Bpm => ("bpm", "Beats per minute"),
        TagField::Isrc => ("isrc", "ISRC recording code"),
        TagField::InitialKey => ("key", "Musical key"),
        TagField::CatalogNumber => ("catalognumber", "Label catalogue number"),
        TagField::Url => ("url", "Release webpage"),
        TagField::MediaType => ("media", "Media type — Vinyl, CD, Cassette, File"),
        TagField::Custom(_) => ("custom", "Custom field — not addressable from a mask"),
    }
}

/// What a file placeholder is called and what it holds (#148). Exhaustive over
/// [`FileValue`] for the same reason [`tag_placeholder_doc`] is over
/// [`TagField`].
fn file_placeholder_doc(value: FileValue) -> (&'static str, &'static str) {
    match value {
        FileValue::Name => ("filename", "File name without the extension"),
        FileValue::Ext => ("fileext", "Extension alone, no dot"),
        FileValue::NameExt => ("filenameext", "File name with the extension"),
        FileValue::Path => ("filepath", "Full path (separators stripped)"),
        FileValue::Folder(1) => ("foldername", "Containing folder"),
        FileValue::Folder(2) => ("foldername2", "The folder above that"),
        FileValue::Folder(_) => ("foldername3", "Two folders above"),
        FileValue::Length => ("_length", "Duration, m:ss"),
        FileValue::LengthSec => ("_length_sec", "Duration in seconds"),
        FileValue::Bitrate => ("_bitrate", "Bitrate, kbps"),
        FileValue::SampleRate => ("_samplerate", "Sample rate, Hz"),
        FileValue::Channels => ("_channels", "Channel count"),
        FileValue::Codec => ("_codec", "Container — MP3, FLAC, APE"),
        FileValue::FileSize => ("_filesize", "Size, human-readable"),
        FileValue::FileSizeBytes => ("_filesize_bytes", "Size in bytes"),
        FileValue::FileDate => ("_filedate", "Modified date, YYYY-MM-DD"),
    }
}

/// One entry in the placeholder reference (#148).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceholderDoc {
    /// How it is written in a pattern, ready to drop at a caret: a placeholder
    /// wears its percent signs, a function its parentheses with a slot per
    /// required argument — `$substr(,,)` (#73). The skeleton is deliberately
    /// valid to parse, so inserting one never leaves a pattern the parser
    /// rejects; it renders empty until the arguments are typed in.
    pub token: String,
    /// The bare name; for a placeholder, what goes between the percent signs.
    pub name: &'static str,
    pub description: &'static str,
    /// Which section of the reference it belongs under.
    pub group: PlaceholderGroup,
    /// Whether it can be used to build a name from tags.
    pub render: bool,
    /// Whether it can be used to read tags out of a name.
    pub extract: bool,
}

/// How the reference groups placeholders (#148).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderGroup {
    /// A tag field.
    Tag,
    /// Where the file lives.
    File,
    /// A property of the audio.
    Technical,
    /// `%side%` and `%skip%`, which behave unlike the rest.
    Special,
    /// A `$name(…)` call rather than a placeholder (#73).
    Function,
}

impl PlaceholderGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tag => "Tags",
            Self::File => "File",
            Self::Technical => "Technical",
            Self::Special => "Special",
            Self::Function => "Functions",
        }
    }
}

/// Every placeholder the parser accepts, in reference order (#148).
///
/// This is what the in-app reference renders, and it comes off the same tables
/// the parser reads — a name shown here is by construction a name that parses.
/// `Custom` is excluded because it isn't addressable from a mask.
pub fn placeholder_reference() -> Vec<PlaceholderDoc> {
    let tag_order = [
        TagField::Artist,
        TagField::Title,
        TagField::Album,
        TagField::AlbumArtist,
        TagField::TrackNumber,
        TagField::TrackTotal,
        TagField::DiscNumber,
        TagField::DiscTotal,
        TagField::Year,
        TagField::Genre,
        TagField::Comment,
        TagField::Composer,
        TagField::Publisher,
        TagField::Bpm,
        TagField::Isrc,
        TagField::InitialKey,
        TagField::CatalogNumber,
        TagField::Url,
        TagField::MediaType,
    ];
    let file_order = [
        FileValue::Name,
        FileValue::Ext,
        FileValue::NameExt,
        FileValue::Path,
        FileValue::Folder(1),
        FileValue::Folder(2),
        FileValue::Folder(3),
        FileValue::Length,
        FileValue::LengthSec,
        FileValue::Bitrate,
        FileValue::SampleRate,
        FileValue::Channels,
        FileValue::Codec,
        FileValue::FileSize,
        FileValue::FileSizeBytes,
        FileValue::FileDate,
    ];

    let mut docs: Vec<PlaceholderDoc> = tag_order
        .iter()
        .map(|field| {
            let (name, description) = tag_placeholder_doc(field);
            PlaceholderDoc {
                token: format!("%{name}%"),
                name,
                description,
                group: PlaceholderGroup::Tag,
                render: true,
                extract: true,
            }
        })
        .collect();
    docs.extend(file_order.into_iter().map(|value| {
        let (name, description) = file_placeholder_doc(value);
        PlaceholderDoc {
            token: format!("%{name}%"),
            name,
            description,
            // The underscore that marks a technical value in the pattern marks
            // it here too, so the split can't drift from the spelling.
            group: if name.starts_with('_') {
                PlaceholderGroup::Technical
            } else {
                PlaceholderGroup::File
            },
            // Render-only, every one of them: there is no tag to read a bitrate
            // or a folder name back into (#147).
            render: true,
            extract: false,
        }
    }));
    docs.push(PlaceholderDoc {
        token: "%side%".to_string(),
        name: "side",
        description: "Vinyl side letter, from the disc number",
        group: PlaceholderGroup::Special,
        render: true,
        extract: false,
    });
    docs.push(PlaceholderDoc {
        token: "%skip%".to_string(),
        name: "skip",
        description: "Matches and discards a run of text",
        group: PlaceholderGroup::Special,
        render: false,
        extract: true,
    });
    // The function library (#73), off the same table the parser reads — a name
    // shown here is by construction a name that parses, exactly as for the
    // placeholders above. All render-only: a function transforms, and a
    // transformation cannot be undone out of a filename.
    docs.extend(ALL_FUNCTIONS.iter().map(|function| {
        let (name, description) = function.doc();
        PlaceholderDoc {
            token: function.token(),
            name,
            description,
            group: PlaceholderGroup::Function,
            render: true,
            extract: false,
        }
    }));
    docs
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
    #[error("render-only pattern: computed values and functions cannot be extracted")]
    RenderOnly,
    #[error("unknown function: ${0}")]
    UnknownFunction(String),
    #[error("unclosed function call: ${0}( is missing its )")]
    UnclosedCall(String),
    #[error("${name} takes {expected} arguments, not {actual}")]
    BadArity {
        name: &'static str,
        expected: String,
        actual: usize,
    },
    #[error("bad argument: {0}")]
    BadArgument(String),
    #[error("extract-only pattern: %skip% discards text and cannot be rendered")]
    ExtractOnly,
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

    // #140: the same pair, with the widths written out, says exactly where
    // "101" divides — so it extracts, and still renders the same.
    #[test]
    fn a_stated_width_splits_adjacent_placeholders() {
        let mask = Mask::parse("%disc:1%%track:2%_%artist%_-_%title%").unwrap();
        let tags = mask.extract("101_the_x_factor_-_desert_rain").unwrap();
        assert_eq!(tags.get(&TagField::DiscNumber).unwrap(), "1");
        assert_eq!(tags.get(&TagField::TrackNumber).unwrap(), "01");
        assert_eq!(tags.get(&TagField::Artist).unwrap(), "the_x_factor");
        assert_eq!(tags.get(&TagField::Title).unwrap(), "desert_rain");

        // Render is unchanged: the width is still a padding minimum there.
        let mut out = TagMap::new();
        out.insert(TagField::DiscNumber, "1".into());
        out.insert(TagField::TrackNumber, "1".into());
        out.insert(TagField::Artist, "the_x_factor".into());
        out.insert(TagField::Title, "desert_rain".into());
        assert_eq!(mask.render(&out).unwrap(), "101_the_x_factor_-_desert_rain");
    }

    // Only a width the pattern states is a fixed length. `%track%` defaults to
    // two for padding, and if that counted as a match length this name — with a
    // one-digit track — would stop extracting.
    #[test]
    fn a_default_width_is_not_a_match_length() {
        let mask = Mask::parse("%track% - %title%").unwrap();
        let tags = mask.extract("5 - Roygbiv").unwrap();
        assert_eq!(tags.get(&TagField::TrackNumber).unwrap(), "5");
        assert_eq!(tags.get(&TagField::Title).unwrap(), "Roygbiv");
    }

    // A stated width on an integer field matches digits, so a name that carries
    // something else there misses cleanly instead of handing the writer two
    // letters as a track number.
    #[test]
    fn a_stated_width_on_an_integer_field_matches_digits_only() {
        let mask = Mask::parse("%track:2% %title%").unwrap();
        assert_eq!(
            mask.extract("07 Roygbiv")
                .unwrap()
                .get(&TagField::TrackNumber),
            Some(&"07".to_string())
        );
        assert!(matches!(
            mask.extract("A1 Roygbiv"),
            Err(MaskError::NoMatch)
        ));

        // A text field takes any run of exactly that length.
        let text = Mask::parse("%genre:4% - %title%").unwrap();
        assert_eq!(
            text.extract("Rock - Roygbiv")
                .unwrap()
                .get(&TagField::Genre),
            Some(&"Rock".to_string())
        );
    }

    #[test]
    fn skip_discards_a_trailing_junk_run() {
        // A junk suffix (source tag, release-group noise) maps to no field (#70).
        let mask = Mask::parse("%artist% - %title% %skip%").unwrap();
        let extracted = mask.extract("Aphex Twin - Xtal [promo]").unwrap();
        assert_eq!(extracted.get(&TagField::Artist).unwrap(), "Aphex Twin");
        assert_eq!(extracted.get(&TagField::Title).unwrap(), "Xtal");
        assert_eq!(extracted.len(), 2); // nothing captured for %skip%
    }

    #[test]
    fn skip_discards_a_leading_run_and_may_repeat() {
        let mask = Mask::parse("%skip% - %title% - %skip%").unwrap();
        let extracted = mask.extract("junk - Xtal - more junk here").unwrap();
        assert_eq!(extracted.get(&TagField::Title).unwrap(), "Xtal");
        assert_eq!(extracted.len(), 1);
    }

    #[test]
    fn skip_is_case_insensitive() {
        let mask = Mask::parse("%SKIP% - %title%").unwrap();
        let extracted = mask.extract("whatever - Xtal").unwrap();
        assert_eq!(extracted.get(&TagField::Title).unwrap(), "Xtal");
    }

    #[test]
    fn skip_mask_is_extract_only_so_render_refuses_it() {
        let mask = Mask::parse("%artist% %skip%").unwrap();
        assert!(matches!(
            mask.render(&tags(&[(TagField::Artist, "Aphex Twin")])),
            Err(MaskError::ExtractOnly)
        ));
    }

    #[test]
    fn skip_adjacent_to_a_placeholder_cannot_be_extracted() {
        // `%skip%%title%` has no boundary between the discard and the field.
        let mask = Mask::parse("%skip%%title%").unwrap();
        assert!(matches!(
            mask.extract("junkXtal"),
            Err(MaskError::Ambiguous)
        ));
    }

    #[test]
    fn side_renders_a_letter_for_vinyl_and_nothing_for_other_media() {
        let mask = Mask::parse("%side%%track:1% - %title%").unwrap();
        // Vinyl: disc 2 -> side B, so "B3".
        let vinyl = tags(&[
            (TagField::MediaType, "Vinyl"),
            (TagField::DiscNumber, "2"),
            (TagField::TrackNumber, "3"),
            (TagField::Title, "Rose"),
        ]);
        assert_eq!(mask.render(&vinyl).unwrap(), "B3 - Rose");
        // CD: no side letter, just the track number.
        let cd = tags(&[
            (TagField::MediaType, "CD"),
            (TagField::DiscNumber, "2"),
            (TagField::TrackNumber, "3"),
            (TagField::Title, "Rose"),
        ]);
        assert_eq!(mask.render(&cd).unwrap(), "3 - Rose");
    }

    #[test]
    fn side_makes_a_mask_render_only() {
        let mask = Mask::parse("%side% %title%").unwrap();
        assert!(matches!(mask.extract("A Rose"), Err(MaskError::RenderOnly)));
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

    // ---- file and technical placeholders (#147) ----

    fn file_at(path: &Path) -> FileContext<'_> {
        FileContext {
            path: Some(path),
            ..FileContext::default()
        }
    }

    #[test]
    fn renders_the_path_placeholders() {
        let path = Path::new("/music/Boards of Canada/Music Has the Right/03 Roygbiv.flac");
        let file = file_at(path);
        let render = |pattern: &str| {
            Mask::parse(pattern)
                .unwrap()
                .render_with(&TagMap::new(), &file)
                .unwrap()
        };
        assert_eq!(render("%filename%"), "03 Roygbiv");
        assert_eq!(render("%fileext%"), "flac");
        assert_eq!(render("%filenameext%"), "03 Roygbiv.flac");
        assert_eq!(render("%foldername%"), "Music Has the Right");
        assert_eq!(render("%foldername2%"), "Boards of Canada");
        assert_eq!(render("%foldername3%"), "music");
    }

    #[test]
    fn a_folder_level_above_the_root_renders_empty() {
        // Two levels up from a file one deep is past the root: a thin result,
        // not a broken pattern.
        let file = file_at(Path::new("/track.flac"));
        let mask = Mask::parse("%foldername2%").unwrap();
        assert_eq!(mask.render_with(&TagMap::new(), &file).unwrap(), "");
    }

    #[test]
    fn the_full_path_is_sanitized_like_any_other_value() {
        // It lands in a filename like everything else, so its separators are
        // stripped -- it identifies the file, it doesn't reconstruct its path.
        let file = file_at(Path::new("/music/x/track.flac"));
        let rendered = Mask::parse("%filepath%")
            .unwrap()
            .render_with(&TagMap::new(), &file)
            .unwrap();
        assert!(!rendered.contains('/'), "got {rendered}");
        assert!(rendered.contains("track.flac"), "got {rendered}");
    }

    #[test]
    fn renders_the_technical_placeholders() {
        let path = Path::new("/music/track.mp3");
        let file = FileContext {
            path: Some(path),
            format: Some(AudioFormat::Mp3),
            props: Some(AudioProps {
                duration_secs: 3812,
                bitrate_kbps: Some(320),
                sample_rate_hz: Some(44_100),
                channels: Some(2),
            }),
            size_bytes: Some(7_654_321),
            modified: Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)),
        };
        let render = |pattern: &str| {
            Mask::parse(pattern)
                .unwrap()
                .render_with(&TagMap::new(), &file)
                .unwrap()
        };
        assert_eq!(render("%_length%"), "1:03:32");
        assert_eq!(render("%_length_sec%"), "3812");
        assert_eq!(render("%_bitrate%"), "320");
        assert_eq!(render("%_samplerate%"), "44100");
        assert_eq!(render("%_channels%"), "2");
        assert_eq!(render("%_codec%"), "MP3");
        assert_eq!(render("%_filesize%"), "7.3 MB");
        assert_eq!(render("%_filesize_bytes%"), "7654321");
        assert_eq!(render("%_filedate%"), "2023-11-14");
    }

    #[test]
    fn a_file_placeholder_without_a_context_renders_empty() {
        // Same contract as `%side%` on a CD: an absent value is a valid outcome,
        // never an error -- and it contributes nothing to a conditional section.
        let mask = Mask::parse("%title%[ [%_bitrate%kbps]]").unwrap();
        let rendered = mask.render(&tags(&[(TagField::Title, "Roygbiv")])).unwrap();
        assert_eq!(rendered, "Roygbiv");
    }

    #[test]
    fn a_file_placeholder_makes_the_mask_render_only() {
        // There is no tag to extract a bitrate or a folder name into.
        let mask = Mask::parse("%foldername%/%title%").unwrap();
        assert!(matches!(
            mask.extract("Album/Roygbiv"),
            Err(MaskError::RenderOnly)
        ));
    }

    #[test]
    fn only_the_patterns_that_ask_for_them_need_a_probe_or_metadata() {
        // The guard that keeps the common tags-only mask from paying for a
        // second read of every file.
        let plain = Mask::parse("%artist% - %title%").unwrap();
        assert!(!plain.needs_audio_props());
        assert!(!plain.needs_metadata());

        let named = Mask::parse("%foldername% - %filename%").unwrap();
        assert!(!named.needs_audio_props());
        assert!(!named.needs_metadata());

        let technical = Mask::parse("%title% [%_bitrate%]").unwrap();
        assert!(technical.needs_audio_props());
        assert!(!technical.needs_metadata());

        let dated = Mask::parse("%title% [%_filedate%]").unwrap();
        assert!(!dated.needs_audio_props());
        assert!(dated.needs_metadata());
    }

    #[test]
    fn a_width_on_a_file_placeholder_is_not_a_placeholder() {
        // A width zero-pads a number; none of these are numbers to pad, so the
        // spec is rejected rather than silently ignoring the width.
        // Reported by name, the same way `%bogus:5%` is.
        assert!(matches!(
            Mask::parse("%filename:5%"),
            Err(MaskError::UnknownPlaceholder(name)) if name == "filename"
        ));
    }

    #[test]
    fn file_placeholder_names_are_case_insensitive() {
        let file = file_at(Path::new("/music/track.flac"));
        let mask = Mask::parse("%FileName%").unwrap();
        assert_eq!(mask.render_with(&TagMap::new(), &file).unwrap(), "track");
    }

    // ---- the function library (#73) ----

    #[test]
    fn a_function_transforms_what_the_placeholders_resolved_to() {
        let mask = Mask::parse("$upper(%artist%) - $caps(%title%)").unwrap();
        let tags = tags(&[
            (TagField::Artist, "autechre"),
            (TagField::Title, "second bad vilbel"),
        ]);
        assert_eq!(mask.render(&tags).unwrap(), "AUTECHRE - Second Bad Vilbel");
    }

    #[test]
    fn calls_nest_and_an_argument_is_a_pattern_of_its_own() {
        // The argument holds a placeholder, a literal and another call.
        let mask = Mask::parse("$left($upper(%artist% '('%year%')'),12)").unwrap();
        let tags = tags(&[(TagField::Artist, "autechre"), (TagField::Year, "1994")]);
        assert_eq!(mask.render(&tags).unwrap(), "AUTECHRE (19");
    }

    #[test]
    fn a_comma_or_a_bracket_inside_an_argument_is_written_quoted() {
        let mask = Mask::parse("$replace(%artist%,'&',and)").unwrap();
        let ampersand = tags(&[(TagField::Artist, "Simon & Garfunkel")]);
        assert_eq!(mask.render(&ampersand).unwrap(), "Simon and Garfunkel");

        // `,` ends an argument, so a literal one has to be quoted.
        let mask = Mask::parse("$getpart(%artist%,',',1)").unwrap();
        let sorted = tags(&[(TagField::Artist, "Beatles, The")]);
        assert_eq!(mask.render(&sorted).unwrap(), "Beatles");
    }

    #[test]
    fn a_section_inside_a_call_still_disappears_when_it_is_empty() {
        let mask = Mask::parse("$upper(%artist%[ - %year%])").unwrap();
        let with_year = tags(&[(TagField::Artist, "Autechre"), (TagField::Year, "1994")]);
        assert_eq!(mask.render(&with_year).unwrap(), "AUTECHRE - 1994");
        let without = tags(&[(TagField::Artist, "Autechre")]);
        assert_eq!(mask.render(&without).unwrap(), "AUTECHRE");
    }

    #[test]
    fn a_call_inside_a_section_decides_whether_the_section_survives() {
        let mask = Mask::parse("%title%[ '['$upper(%key%)']']").unwrap();
        let with_key = tags(&[(TagField::Title, "Rain"), (TagField::InitialKey, "am")]);
        assert_eq!(mask.render(&with_key).unwrap(), "Rain [AM]");
        // No key: the call produces nothing, so the section contributes nothing
        // -- not even its brackets and its space.
        let without = tags(&[(TagField::Title, "Rain")]);
        assert_eq!(mask.render(&without).unwrap(), "Rain");
    }

    #[test]
    fn a_missing_tag_is_as_fatal_inside_a_call_as_outside_one() {
        // Wrapping a placeholder in a function must not quietly turn an
        // unsatisfiable pattern into an empty string -- that is what `[…]` is
        // for, and it still works inside a call.
        let mask = Mask::parse("$upper(%artist%)").unwrap();
        assert!(matches!(
            mask.render(&TagMap::new()),
            Err(MaskError::MissingTag(name)) if name == "artist"
        ));
        let optional = Mask::parse("[$upper(%artist%)]").unwrap();
        assert_eq!(optional.render(&TagMap::new()).unwrap(), "");
    }

    #[test]
    fn a_dollar_that_starts_no_call_is_an_ordinary_character() {
        // The sigil arrived after the grammar shipped, so a pattern that merely
        // contains a `$` has to keep meaning what it meant.
        let mask = Mask::parse("%title% '['$5']'").unwrap();
        let tags = tags(&[(TagField::Title, "Rain")]);
        assert_eq!(mask.render(&tags).unwrap(), "Rain [$5]");
        let bare = Mask::parse("$").unwrap();
        assert_eq!(bare.render(&TagMap::new()).unwrap(), "$");
    }

    #[test]
    fn a_misspelt_or_miscounted_call_is_refused_when_the_pattern_is_parsed() {
        // Not at render time: the pattern is typed with a live preview beside
        // it, and "unknown function" there beats a wrong filename per file.
        assert!(matches!(
            Mask::parse("$upprer(%title%)"),
            Err(MaskError::UnknownFunction(name)) if name == "upprer"
        ));
        assert!(matches!(
            Mask::parse("$left(%title%)"),
            Err(MaskError::BadArity {
                name: "left",
                actual: 1,
                ..
            })
        ));
        assert!(matches!(
            Mask::parse("$upper(%title%,2)"),
            Err(MaskError::BadArity {
                name: "upper",
                actual: 2,
                ..
            })
        ));
        assert!(matches!(
            Mask::parse("$upper(%title%"),
            Err(MaskError::UnclosedCall(name)) if name == "upper"
        ));
    }

    #[test]
    fn an_argument_that_should_be_a_number_and_is_not_fails_the_render() {
        let mask = Mask::parse("$left(%title%,%artist%)").unwrap();
        let tags = tags(&[(TagField::Title, "Rain"), (TagField::Artist, "Autechre")]);
        assert!(matches!(mask.render(&tags), Err(MaskError::BadArgument(_))));
    }

    #[test]
    fn a_mask_that_calls_a_function_cannot_extract() {
        // A substitution is invertible and that is what the two directions rest
        // on; a transformation is not, and guessing which half of `THE BEATLES`
        // the pattern upper-cased is exactly the invention this module refuses.
        let mask = Mask::parse("$upper(%artist%) - %title%").unwrap();
        assert!(matches!(
            mask.extract("AUTECHRE - Rain"),
            Err(MaskError::RenderOnly)
        ));
        // Including when the call is buried in a section.
        let nested = Mask::parse("%title%[ $upper(%key%)]").unwrap();
        assert!(matches!(nested.extract("Rain"), Err(MaskError::RenderOnly)));
    }

    #[test]
    fn a_file_property_inside_a_call_still_asks_for_the_read_it_needs() {
        // Missing this would leave the render without the probe, and
        // `$upper(%_codec%)` would quietly come out empty on every file.
        let bitrate = Mask::parse("$num(%_bitrate%,4)").unwrap();
        assert!(bitrate.needs_audio_props(), "the probe was skipped");
        let dated = Mask::parse("[$left(%_filedate%,4)]").unwrap();
        assert!(dated.needs_metadata(), "the metadata read was skipped");
        // And a mask that asks for none of it still costs nothing extra.
        let plain = Mask::parse("$upper(%artist%)").unwrap();
        assert!(!plain.needs_audio_props() && !plain.needs_metadata());
    }

    #[test]
    fn the_two_spellings_of_padding_a_number_agree() {
        // `$num` exists to unify with `%field:width%`, so the two had better
        // produce the same thing -- including on a value that is not a number,
        // which neither may corrupt.
        let tags = tags(&[(TagField::TrackNumber, "7"), (TagField::DiscNumber, "A1")]);
        assert_eq!(
            Mask::parse("%track:3%").unwrap().render(&tags).unwrap(),
            Mask::parse("$num(%track%,3)")
                .unwrap()
                .render(&tags)
                .unwrap()
        );
        assert_eq!(
            Mask::parse("%disc:3%").unwrap().render(&tags).unwrap(),
            Mask::parse("$num(%disc%,3)")
                .unwrap()
                .render(&tags)
                .unwrap()
        );
    }

    #[test]
    fn the_real_thing_a_function_library_is_for() {
        // The case that made this worth building: a sorting-form folder name
        // from an album artist, with the throwaway attribute off the title.
        let mask = Mask::parse("$swapprefix(%albumartist%)/$cutmix(%title%)").unwrap();
        let tags = tags(&[
            (TagField::AlbumArtist, "The Beatles"),
            (TagField::Title, "Come Together (Remastered)"),
        ]);
        assert_eq!(mask.render(&tags).unwrap(), "Beatles, The/Come Together");
    }

    // ---- the placeholder reference (#148) ----

    #[test]
    fn every_documented_placeholder_actually_parses() {
        // The point of the reference is that what it shows is what the parser
        // takes. A name in the list that doesn't parse would be worse than no
        // list at all -- it would send the user down exactly the guessing path
        // the reference exists to end.
        // Written the way the reference offers it, which is also the way the
        // in-app list inserts it -- so this pins the insert too, not just the
        // name (#73).
        for doc in placeholder_reference() {
            let pattern = doc.token.clone();
            assert!(
                Mask::parse(&pattern).is_ok(),
                "the reference lists {pattern}, which the parser rejects"
            );
        }
    }

    #[test]
    fn the_reference_agrees_with_each_placeholders_direction() {
        // A direction is refused by exactly one error each: everything else a
        // single-placeholder mask can return (a missing tag, no match) is about
        // the data, not about what the placeholder is capable of.
        for doc in placeholder_reference() {
            let mask = Mask::parse(&doc.token).unwrap();
            let renders = !matches!(mask.render(&TagMap::new()), Err(MaskError::ExtractOnly));
            let extracts = !matches!(mask.extract("value"), Err(MaskError::RenderOnly));
            assert_eq!(renders, doc.render, "{} renders differently", doc.name);
            assert_eq!(extracts, doc.extract, "{} extracts differently", doc.name);
        }
    }

    #[test]
    fn the_reference_has_no_duplicate_names() {
        let mut names: Vec<&str> = placeholder_reference().iter().map(|d| d.name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "a placeholder is listed twice");
    }

    #[test]
    fn the_technical_group_is_exactly_the_underscored_names() {
        for doc in placeholder_reference() {
            let technical = doc.group == PlaceholderGroup::Technical;
            assert_eq!(
                technical,
                doc.name.starts_with('_'),
                "{} is grouped against its spelling",
                doc.name
            );
        }
    }

    #[test]
    fn formats_durations_sizes_and_dates() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(9), "0:09");
        assert_eq!(format_duration(212), "3:32");
        assert_eq!(format_duration(3600), "1:00:00");

        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");

        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(59), (1970, 3, 1));
        // A leap day, the case the conversion is easiest to get wrong on.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }
}
