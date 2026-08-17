//! The mask function library (#73).
//!
//! A mask is a small expression language: `$name(arg,arg)` around the
//! placeholders and conditional sections the grammar already had. This module
//! is the library itself — the names, how many arguments each takes, and what
//! each one does to a string. The grammar that gets here (parsing `$name(...)`,
//! evaluating the arguments) lives in the parent module.
//!
//! Three rules hold across every function, and they are what keep a mask from
//! failing over ordinary data:
//!
//! - **Positions and lengths are in characters, never bytes, and are 1-based.**
//!   A library is full of non-ASCII titles; `$left(%title%,3)` must cut three
//!   letters off `Étude`, not three bytes into the middle of one.
//! - **Out of range clamps, it doesn't fail.** `$left` of a short title is the
//!   whole title. A rename over a thousand files must not stop because one of
//!   them has a two-letter name.
//! - **What fails is a bad *pattern*, not bad data** — a non-numeric argument
//!   where a count belongs is a mistake in the mask, and it says so.
//!
//! Whitespace-sensitive functions are the exception worth naming: `$trim` and
//! friends work on the Unicode definition of whitespace, so a non-breaking space
//! copied out of a web page is trimmed like any other.

use super::{pad_numeric, MaskError};
use crate::matching::NOISE_ATTRIBUTES;

/// The default leading words [`Function::StripPrefix`] and
/// [`Function::SwapPrefix`] remove, when the pattern names none of its own.
/// Deliberately just the English articles: the same three the matcher strips
/// when it compares titles.
const DEFAULT_PREFIXES: &[&str] = &["the", "a", "an"];

/// One function of the mask language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Function {
    Lower,
    Upper,
    Caps,
    Caps2,
    Left,
    Right,
    Substr,
    Trim,
    TrimLeft,
    TrimRight,
    PadLeft,
    PadRight,
    Num,
    Len,
    StrPos,
    Replace,
    Insert,
    Reverse,
    GetPart,
    StripPrefix,
    SwapPrefix,
    CutMix,
}

impl Function {
    /// The name as written in a pattern, without the `$`.
    pub(super) fn name(self) -> &'static str {
        self.doc().0
    }

    /// How many arguments the function takes: the minimum, and the maximum —
    /// `None` where it takes any number beyond the minimum ([`StripPrefix`] and
    /// [`SwapPrefix`] accept a list of words).
    ///
    /// Checked when the pattern is **parsed**, not when it renders, so a
    /// miscounted call is caught while it is being typed rather than once per
    /// file in the middle of a rename.
    ///
    /// [`StripPrefix`]: Self::StripPrefix
    /// [`SwapPrefix`]: Self::SwapPrefix
    pub(super) fn arity(self) -> (usize, Option<usize>) {
        match self {
            Self::Lower
            | Self::Upper
            | Self::Caps
            | Self::Caps2
            | Self::Trim
            | Self::TrimLeft
            | Self::TrimRight
            | Self::Len
            | Self::Reverse
            | Self::CutMix => (1, Some(1)),
            Self::Left | Self::Right | Self::Num | Self::StrPos => (2, Some(2)),
            Self::PadLeft | Self::PadRight => (2, Some(3)),
            Self::Substr | Self::Replace | Self::Insert | Self::GetPart => (3, Some(3)),
            Self::StripPrefix | Self::SwapPrefix => (1, None),
        }
    }

    /// Apply the function to its already-evaluated arguments.
    ///
    /// The arity is guaranteed by the parser, so indexing `args` past 0 is safe
    /// wherever [`arity`](Self::arity) demands it.
    pub(super) fn apply(self, args: &[String]) -> Result<String, MaskError> {
        let first = args[0].as_str();
        Ok(match self {
            Self::Lower => first.to_lowercase(),
            Self::Upper => first.to_uppercase(),
            Self::Caps => capitalize(first, true),
            Self::Caps2 => capitalize(first, false),
            Self::Left => take_chars(first, count(self, &args[1])?, false),
            Self::Right => take_chars(first, count(self, &args[1])?, true),
            Self::Substr => {
                let from = count(self, &args[1])?;
                let to = count(self, &args[2])?;
                substr(first, from, to)
            }
            Self::Trim => first.trim().to_string(),
            Self::TrimLeft => first.trim_start().to_string(),
            Self::TrimRight => first.trim_end().to_string(),
            Self::PadLeft => pad(first, count(self, &args[1])?, args.get(2), true),
            Self::PadRight => pad(first, count(self, &args[1])?, args.get(2), false),
            // The same rule `%track:3%` follows, so the two spellings of "pad a
            // number" cannot drift: a value that is not all digits is left
            // alone, because zero-padding `A1` or `1/12` would corrupt it.
            Self::Num => pad_numeric(first, count(self, &args[1])?).into_owned(),
            Self::Len => first.chars().count().to_string(),
            Self::StrPos => str_pos(first, &args[1]).to_string(),
            // An empty needle would otherwise match between every character and
            // splice the replacement through the whole value.
            Self::Replace => {
                if args[1].is_empty() {
                    first.to_string()
                } else {
                    first.replace(args[1].as_str(), &args[2])
                }
            }
            Self::Insert => insert(first, &args[1], count(self, &args[2])?),
            Self::Reverse => first.chars().rev().collect(),
            Self::GetPart => get_part(first, count(self, &args[2])?, &args[1]),
            Self::StripPrefix => match split_prefix(first, &args[1..]) {
                Some((_, rest)) => rest.to_string(),
                None => first.to_string(),
            },
            // "The Beatles" -> "Beatles, The", the sorting form. A value with no
            // matching prefix is returned untouched rather than gaining a comma.
            Self::SwapPrefix => match split_prefix(first, &args[1..]) {
                Some((prefix, rest)) => format!("{rest}, {prefix}"),
                None => first.to_string(),
            },
            Self::CutMix => cut_mix(first),
        })
    }

    /// The call skeleton the reference offers for insertion: the name, and a
    /// comma for every required argument beyond the first, so what lands in the
    /// pattern already parses and already shows how many slots there are.
    /// `$substr(,,)` is three empty arguments waiting to be typed into.
    pub(super) fn token(self) -> String {
        let (minimum, _) = self.arity();
        format!(
            "${}({})",
            self.name(),
            ",".repeat(minimum.saturating_sub(1))
        )
    }

    /// The name and the one-line description the in-app reference shows (#148).
    ///
    /// Exhaustive on purpose, exactly like the placeholder tables: a function
    /// cannot reach the parser without someone deciding what it is called and
    /// what the reference says about it.
    pub(super) fn doc(self) -> (&'static str, &'static str) {
        match self {
            Self::Lower => ("lower", "$lower(x) — lower case"),
            Self::Upper => ("upper", "$upper(x) — UPPER CASE"),
            Self::Caps => ("caps", "$caps(x) — Capitalise Each Word, Lowering The Rest"),
            Self::Caps2 => (
                "caps2",
                "$caps2(x) — Capitalise Each Word, Leaving The REST",
            ),
            Self::Left => ("left", "$left(x,n) — the first n characters"),
            Self::Right => ("right", "$right(x,n) — the last n characters"),
            Self::Substr => (
                "substr",
                "$substr(x,from,to) — characters from..to, 1-based",
            ),
            Self::Trim => ("trim", "$trim(x) — drop surrounding whitespace"),
            Self::TrimLeft => ("trimleft", "$trimleft(x) — drop leading whitespace"),
            Self::TrimRight => ("trimright", "$trimright(x) — drop trailing whitespace"),
            Self::PadLeft => (
                "padleft",
                "$padleft(x,n[,c]) — pad on the left to n characters",
            ),
            Self::PadRight => (
                "padright",
                "$padright(x,n[,c]) — pad on the right to n characters",
            ),
            Self::Num => ("num", "$num(x,n) — zero-pad a number, as %track:n% does"),
            Self::Len => ("len", "$len(x) — how many characters"),
            Self::StrPos => (
                "strpos",
                "$strpos(x,find) — where find starts, 1-based, 0 if absent",
            ),
            Self::Replace => ("replace", "$replace(x,find,with) — every occurrence"),
            Self::Insert => ("insert", "$insert(x,y,n) — put y at position n"),
            Self::Reverse => ("reverse", "$reverse(x) — characters back to front"),
            Self::GetPart => (
                "getpart",
                "$getpart(x,sep,n) — the nth piece after splitting on sep",
            ),
            Self::StripPrefix => (
                "stripprefix",
                "$stripprefix(x[,word…]) — drop a leading The/A/An",
            ),
            Self::SwapPrefix => (
                "swapprefix",
                "$swapprefix(x[,word…]) — The Beatles -> Beatles, The",
            ),
            Self::CutMix => (
                "cutmix",
                "$cutmix(x) — drop a trailing (Original Mix), (Remastered), …",
            ),
        }
    }
}

/// Every function, in reference order — grouped the way someone reading the list
/// would look for them rather than alphabetically.
pub(super) const ALL_FUNCTIONS: &[Function] = &[
    Function::Lower,
    Function::Upper,
    Function::Caps,
    Function::Caps2,
    Function::Trim,
    Function::TrimLeft,
    Function::TrimRight,
    Function::Left,
    Function::Right,
    Function::Substr,
    Function::Len,
    Function::StrPos,
    Function::Num,
    Function::PadLeft,
    Function::PadRight,
    Function::Replace,
    Function::Insert,
    Function::Reverse,
    Function::GetPart,
    Function::StripPrefix,
    Function::SwapPrefix,
    Function::CutMix,
];

/// A function by the name written in the pattern, or `None` — the parser then
/// reports it as unknown rather than quietly treating the call as literal text.
///
/// `$cut` is accepted as a second spelling of `$left`, since that is what the
/// language this borrows from calls it.
pub(super) fn function_from_name(name: &str) -> Option<Function> {
    let lowered = name.to_ascii_lowercase();
    if lowered == "cut" {
        return Some(Function::Left);
    }
    ALL_FUNCTIONS
        .iter()
        .copied()
        .find(|function| function.name() == lowered)
}

/// A count argument: a plain non-negative integer.
///
/// This is the one place a function refuses to carry on. Everything else about a
/// mask degrades gracefully, but `$left(%title%,two)` is not a value that came
/// out wrong, it is a pattern that was written wrong, and saying so beats
/// silently picking a number.
fn count(function: Function, value: &str) -> Result<usize, MaskError> {
    value.trim().parse::<usize>().map_err(|_| {
        MaskError::BadArgument(format!(
            "${} needs a whole number, not {value:?}",
            function.name()
        ))
    })
}

/// Upper-case the first letter of each whitespace-separated word. `lower_rest`
/// distinguishes `$caps` (which lowers what follows) from `$caps2` (which leaves
/// it, so `AC/DC` survives).
fn capitalize(value: &str, lower_rest: bool) -> String {
    let mut out = String::with_capacity(value.len());
    let mut at_word_start = true;
    for character in value.chars() {
        if character.is_whitespace() {
            at_word_start = true;
            out.push(character);
        } else if at_word_start {
            at_word_start = false;
            out.extend(character.to_uppercase());
        } else if lower_rest {
            out.extend(character.to_lowercase());
        } else {
            out.push(character);
        }
    }
    out
}

/// The first (or last) `n` characters, or the whole value when it is shorter.
fn take_chars(value: &str, n: usize, from_end: bool) -> String {
    let characters: Vec<char> = value.chars().collect();
    if n >= characters.len() {
        return value.to_string();
    }
    if from_end {
        characters[characters.len() - n..].iter().collect()
    } else {
        characters[..n].iter().collect()
    }
}

/// Characters `from..=to`, 1-based and inclusive, clamped at both ends. A `from`
/// past the end, or a `to` before the `from`, is an empty string rather than an
/// error — a slice that selects nothing is a legitimate outcome.
fn substr(value: &str, from: usize, to: usize) -> String {
    let characters: Vec<char> = value.chars().collect();
    let start = from.max(1) - 1;
    let end = to.min(characters.len());
    if start >= end {
        return String::new();
    }
    characters[start..end].iter().collect()
}

/// Pad to `width` characters with `filler` (a space when the pattern names
/// none). Never truncates: padding is about lining a column up, and a value
/// already wider than the column still has to be readable.
fn pad(value: &str, width: usize, filler: Option<&String>, on_left: bool) -> String {
    let character = filler.and_then(|text| text.chars().next()).unwrap_or(' ');
    let length = value.chars().count();
    if length >= width {
        return value.to_string();
    }
    let padding: String = std::iter::repeat_n(character, width - length).collect();
    if on_left {
        format!("{padding}{value}")
    } else {
        format!("{value}{padding}")
    }
}

/// Where `needle` starts in `haystack`, 1-based, or 0 when it isn't there.
/// Counted in characters, so the answer means the same thing to `$left` and
/// `$substr`.
fn str_pos(haystack: &str, needle: &str) -> usize {
    match haystack.find(needle) {
        Some(byte_index) => haystack[..byte_index].chars().count() + 1,
        None => 0,
    }
}

/// Insert `addition` so that it begins at 1-based character `position`. Past the
/// end it appends, which is what makes `$insert(x,y,$len(x))` mean "at the end"
/// without the pattern having to be exact.
fn insert(value: &str, addition: &str, position: usize) -> String {
    let characters: Vec<char> = value.chars().collect();
    let at = position.max(1) - 1;
    if at >= characters.len() {
        return format!("{value}{addition}");
    }
    let head: String = characters[..at].iter().collect();
    let tail: String = characters[at..].iter().collect();
    format!("{head}{addition}{tail}")
}

/// The `index`-th piece (1-based) of `value` split on `separator`. Out of range
/// is empty; an empty separator makes the whole value the only piece.
fn get_part(value: &str, index: usize, separator: &str) -> String {
    if index == 0 {
        return String::new();
    }
    if separator.is_empty() {
        return if index == 1 {
            value.to_string()
        } else {
            String::new()
        };
    }
    value
        .split(separator)
        .nth(index - 1)
        .unwrap_or_default()
        .to_string()
}

/// Split a leading word off `value`: the matched prefix as the value spells it,
/// and the rest. Matching ignores case; the returned prefix keeps the original's
/// so `$swapprefix` puts back what was there rather than a normalised form.
///
/// `words` is what the pattern named, or [`DEFAULT_PREFIXES`] when it named
/// nothing.
fn split_prefix<'a>(value: &'a str, words: &[String]) -> Option<(&'a str, &'a str)> {
    let mut candidates: Vec<&str> = words.iter().map(String::as_str).collect();
    if candidates.is_empty() {
        candidates = DEFAULT_PREFIXES.to_vec();
    }
    for word in candidates {
        let word = word.trim();
        if word.is_empty() {
            continue;
        }
        let (head, rest) = value.split_at_checked(word.len())?;
        if head.eq_ignore_ascii_case(word) && rest.starts_with(' ') {
            return Some((head, rest.trim_start()));
        }
    }
    None
}

/// Drop a trailing throwaway attribute — `(Original Mix)`, `[Remastered]`, or
/// the same phrase bare — keeping the value's own casing.
///
/// The vocabulary is the matcher's [`NOISE_ATTRIBUTES`], shared rather than
/// copied: the list of what carries no identity is one decision, and a remixer
/// credit staying put matters just as much here as it does there. Only a
/// *trailing* group is dropped, and only one, for the same reason — text in the
/// middle of a title is part of it.
fn cut_mix(value: &str) -> String {
    let trimmed = value.trim_end();
    for (open, close) in [('(', ')'), ('[', ']')] {
        if let Some(rest) = trimmed.strip_suffix(close) {
            if let Some(index) = rest.rfind(open) {
                if is_noise(&rest[index + 1..]) {
                    return trim_trailing_separator(&rest[..index]);
                }
            }
        }
    }
    for noise in NOISE_ATTRIBUTES {
        if trimmed.len() >= noise.len() {
            let (head, tail) = trimmed.split_at(trimmed.len() - noise.len());
            if tail.eq_ignore_ascii_case(noise) {
                return trim_trailing_separator(head);
            }
        }
    }
    value.to_string()
}

fn is_noise(inner: &str) -> bool {
    let inner = inner.trim().to_lowercase();
    NOISE_ATTRIBUTES.contains(&inner.as_str())
}

/// What is left after cutting an attribute off the end: the separator that used
/// to join it — a dash, a comma, the space before it — goes too, or the value
/// keeps a dangling `Desert Rain -`.
fn trim_trailing_separator(value: &str) -> String {
    value
        .trim_end_matches([' ', '\t', '-', '–', '—', ',', ':'])
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(function: Function, args: &[&str]) -> String {
        let owned: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        function.apply(&owned).expect("applies")
    }

    #[test]
    fn case_functions_differ_on_what_they_do_to_the_rest_of_a_word() {
        assert_eq!(call(Function::Lower, &["AC/DC Live"]), "ac/dc live");
        assert_eq!(call(Function::Upper, &["quiet"]), "QUIET");
        // `$caps` normalises the whole word, `$caps2` only lifts the first
        // letter — which is the one that keeps an acronym readable.
        assert_eq!(call(Function::Caps, &["AC/DC live"]), "Ac/dc Live");
        assert_eq!(call(Function::Caps2, &["AC/DC live"]), "AC/DC Live");
    }

    #[test]
    fn slicing_counts_characters_and_clamps_instead_of_failing() {
        // Bytes would cut this in half; characters do not.
        assert_eq!(call(Function::Left, &["Étude", "3"]), "Étu");
        assert_eq!(call(Function::Right, &["Étude", "2"]), "de");
        // Asking for more than there is yields what there is.
        assert_eq!(call(Function::Left, &["AB", "9"]), "AB");
        assert_eq!(call(Function::Right, &["AB", "9"]), "AB");
        assert_eq!(call(Function::Substr, &["Étude", "2", "4"]), "tud");
        assert_eq!(call(Function::Substr, &["Étude", "4", "99"]), "de");
        // A slice that selects nothing is empty, not an error.
        assert_eq!(call(Function::Substr, &["Étude", "4", "2"]), "");
        assert_eq!(call(Function::Len, &["Étude"]), "5");
    }

    #[test]
    fn padding_never_truncates_and_num_leaves_a_non_number_alone() {
        assert_eq!(call(Function::PadLeft, &["7", "3"]), "  7");
        assert_eq!(call(Function::PadLeft, &["7", "3", "0"]), "007");
        assert_eq!(call(Function::PadRight, &["A", "3", "."]), "A..");
        // Already wide enough: unchanged, not cut down.
        assert_eq!(call(Function::PadLeft, &["long", "2"]), "long");
        // `$num` is the same rule as `%track:3%` — including that it refuses to
        // pad something that isn't a plain number.
        assert_eq!(call(Function::Num, &["7", "3"]), "007");
        assert_eq!(call(Function::Num, &["A1", "3"]), "A1");
        assert_eq!(call(Function::Num, &["1/12", "3"]), "1/12");
    }

    #[test]
    fn finding_and_editing_work_on_characters_too() {
        assert_eq!(call(Function::StrPos, &["Étude No 1", "No"]), "7");
        assert_eq!(call(Function::StrPos, &["Étude", "zz"]), "0");
        assert_eq!(call(Function::Replace, &["a-b-c", "-", " "]), "a b c");
        // An empty needle would otherwise splice the replacement between every
        // character of the value.
        assert_eq!(call(Function::Replace, &["abc", "", "-"]), "abc");
        assert_eq!(call(Function::Insert, &["AC", "/D", "3"]), "AC/D");
        assert_eq!(call(Function::Insert, &["AB", "!", "2"]), "A!B");
        assert_eq!(call(Function::Insert, &["AB", "!", "99"]), "AB!");
        assert_eq!(call(Function::Reverse, &["Étude"]), "edutÉ");
    }

    #[test]
    fn getpart_indexes_from_one_and_is_empty_past_the_end() {
        assert_eq!(call(Function::GetPart, &["A; B; C", "; ", "2"]), "B");
        assert_eq!(call(Function::GetPart, &["A; B", "; ", "5"]), "");
        assert_eq!(call(Function::GetPart, &["A; B", "; ", "0"]), "");
        // No separator to split on: the value is its own only piece.
        assert_eq!(call(Function::GetPart, &["A; B", "", "1"]), "A; B");
    }

    #[test]
    fn the_prefix_functions_share_the_matchers_articles_and_keep_the_casing() {
        assert_eq!(call(Function::StripPrefix, &["The Beatles"]), "Beatles");
        assert_eq!(call(Function::StripPrefix, &["A Tribe"]), "Tribe");
        assert_eq!(call(Function::SwapPrefix, &["The Beatles"]), "Beatles, The");
        // The prefix goes back exactly as the value spelled it.
        assert_eq!(call(Function::SwapPrefix, &["THE Beatles"]), "Beatles, THE");
        // No article: untouched, and no stray comma.
        assert_eq!(call(Function::SwapPrefix, &["Beatles"]), "Beatles");
        assert_eq!(call(Function::StripPrefix, &["Theatre"]), "Theatre");
        // A pattern can name its own words instead.
        assert_eq!(call(Function::StripPrefix, &["Los Lobos", "Los"]), "Lobos");
        assert_eq!(
            call(Function::StripPrefix, &["The Lobos", "Los"]),
            "The Lobos"
        );
    }

    #[test]
    fn cutmix_drops_a_throwaway_attribute_but_never_a_remix_credit() {
        assert_eq!(
            call(Function::CutMix, &["Desert Rain (Original Mix)"]),
            "Desert Rain"
        );
        assert_eq!(
            call(Function::CutMix, &["Desert Rain [Remastered]"]),
            "Desert Rain"
        );
        assert_eq!(
            call(Function::CutMix, &["Desert Rain - Radio Edit"]),
            "Desert Rain"
        );
        // A remixer credit is what distinguishes one recording from another, so
        // it stays — the same rule the matcher follows.
        assert_eq!(
            call(Function::CutMix, &["Desert Rain (Sasha Remix)"]),
            "Desert Rain (Sasha Remix)"
        );
        // Only a trailing attribute, never one in the middle.
        assert_eq!(
            call(Function::CutMix, &["Clean Slate (Sasha Remix)"]),
            "Clean Slate (Sasha Remix)"
        );
        assert_eq!(call(Function::CutMix, &["Desert Rain"]), "Desert Rain");
    }

    #[test]
    fn a_count_argument_that_is_not_a_number_is_a_pattern_error() {
        let args = vec!["title".to_string(), "two".to_string()];
        let error = Function::Left.apply(&args).expect_err("refuses");
        assert!(matches!(error, MaskError::BadArgument(_)), "{error:?}");
    }

    #[test]
    fn every_function_is_reachable_by_the_name_the_reference_shows() {
        for function in ALL_FUNCTIONS {
            assert_eq!(
                function_from_name(function.name()),
                Some(*function),
                "{} is documented but does not parse",
                function.name()
            );
            // The reference's description leads with the call as it is written,
            // so the list doubles as a signature.
            let (name, description) = function.doc();
            assert!(
                description.starts_with(&format!("${name}(")),
                "{name}: {description}"
            );
        }
        // The one accepted alias.
        assert_eq!(function_from_name("cut"), Some(Function::Left));
        assert_eq!(function_from_name("CUT"), Some(Function::Left));
        assert_eq!(function_from_name("nope"), None);
    }
}
