//! Composable text transforms.
//!
//! Used by masks, manual edits and provider post-processing. Deliberately a
//! chain of steps: the "actions"/scripting a tagger is expected to offer
//! later becomes *serialization of chains into saved presets*, not a new
//! subsystem (architecture.md, "Deferred").
//!
//! Every step is a pure `&str -> String`, so a chain is testable without
//! touching a file and can be previewed before anything is written.

use regex::{Regex, RegexBuilder};
use thiserror::Error;

/// A single text transformation over a field value.
pub trait TransformStep: Send + Sync {
    /// Stable identifier for presets and UI.
    fn name(&self) -> &str;
    fn apply(&self, input: &str) -> String;
}

/// An ordered chain of transform steps.
#[derive(Default)]
pub struct TransformChain {
    steps: Vec<Box<dyn TransformStep>>,
}

impl TransformChain {
    pub fn push(&mut self, step: Box<dyn TransformStep>) {
        self.steps.push(step);
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Apply all steps in order.
    pub fn apply(&self, input: &str) -> String {
        self.steps
            .iter()
            .fold(input.to_string(), |acc, step| step.apply(&acc))
    }
}

/// How a [`Replace`] step matches.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReplaceOptions {
    /// Treat the pattern as a regular expression rather than literal text.
    pub regex: bool,
    /// Only match on whole-word boundaries — what stops a `Dj` -> `DJ` rule
    /// from mangling `Djibouti`.
    pub whole_word: bool,
    pub case_sensitive: bool,
}

/// Find-and-replace over a value, literal or regular-expression.
pub struct Replace {
    name: String,
    matcher: Regex,
    replacement: String,
}

impl Replace {
    pub fn new(from: &str, to: &str, options: ReplaceOptions) -> Result<Self, TransformError> {
        if from.is_empty() {
            return Err(TransformError::EmptyPattern);
        }
        let mut pattern = if options.regex {
            from.to_string()
        } else {
            regex::escape(from)
        };
        if options.whole_word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        let matcher = RegexBuilder::new(&pattern)
            .case_insensitive(!options.case_sensitive)
            .build()
            .map_err(|err| TransformError::BadPattern(err.to_string()))?;

        // `$` is a capture reference to the regex engine. In literal mode the
        // user means a dollar sign, so escape it; in regex mode `$1` has to keep
        // working.
        let replacement = if options.regex {
            to.to_string()
        } else {
            to.replace('$', "$$")
        };

        Ok(Self {
            name: format!("replace {from:?} -> {to:?}"),
            matcher,
            replacement,
        })
    }
}

impl TransformStep for Replace {
    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&self, input: &str) -> String {
        self.matcher
            .replace_all(input, &self.replacement)
            .into_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseStyle {
    Lower,
    Upper,
    /// Every Word Capitalised.
    Title,
    /// Only the first word capitalised.
    Sentence,
}

/// Words that must keep their own casing after a case change.
///
/// Blind title-casing is wrong often enough that a repair list is unavoidable:
/// it turns `DJ` into `Dj` and `Symphony III` into `Symphony Iii`. The defaults
/// cover the acronyms and multi-letter roman numerals that actually show up in
/// music metadata, and callers can supply their own list — the right contents
/// are library- and language-specific.
///
/// Single-letter roman numerals (`I`, `V`, `X`, `C`, `D`, `M`) are deliberately
/// absent: `I` is an ordinary word and the rest collide with note names and
/// initials, so forcing them uppercase would do more damage than it repairs.
pub const DEFAULT_CASE_EXCEPTIONS: &[&str] = &[
    "DJ", "MC", "feat", "vs", "CD", "EP", "LP", "DVD", "TV", "OK", "XL", "UK", "USA", "EBM",
    "BDSM", "TNT", "ABBA", "II", "III", "IV", "VI", "VII", "VIII", "IX", "XI", "XII", "XIII",
    "XIV", "XV", "XVI", "XX",
];

pub struct ChangeCase {
    style: CaseStyle,
    exceptions: Vec<String>,
}

impl ChangeCase {
    pub fn new(style: CaseStyle) -> Self {
        Self::with_exceptions(
            style,
            DEFAULT_CASE_EXCEPTIONS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        )
    }

    pub fn with_exceptions(style: CaseStyle, exceptions: Vec<String>) -> Self {
        Self { style, exceptions }
    }

    fn canonical(&self, word: &str) -> Option<&str> {
        self.exceptions
            .iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(word))
            .map(String::as_str)
    }
}

impl TransformStep for ChangeCase {
    fn name(&self) -> &str {
        match self.style {
            CaseStyle::Lower => "lower case",
            CaseStyle::Upper => "UPPER CASE",
            CaseStyle::Title => "Title Case",
            CaseStyle::Sentence => "Sentence case",
        }
    }

    fn apply(&self, input: &str) -> String {
        match self.style {
            // Exceptions would be pointless here: the whole point is that
            // everything ends up in one case.
            CaseStyle::Lower => input.to_lowercase(),
            CaseStyle::Upper => input.to_uppercase(),
            CaseStyle::Title => map_words(input, |word| {
                self.canonical(word)
                    .map(str::to_string)
                    .unwrap_or_else(|| capitalize(word))
            }),
            CaseStyle::Sentence => {
                let mut first = true;
                map_words(input, |word| {
                    let cased = if std::mem::take(&mut first) {
                        capitalize(word)
                    } else {
                        word.to_lowercase()
                    };
                    self.canonical(word).map(str::to_string).unwrap_or(cased)
                })
            }
        }
    }
}

/// Strip accents, leaving the base letter — `Björk` -> `Bjork`.
///
/// A lookup table rather than Unicode normalisation: the set that actually
/// occurs in music metadata is small, and a table makes the behaviour explicit
/// and testable without pulling in a normalisation dependency.
pub struct RemoveDiacritics;

impl TransformStep for RemoveDiacritics {
    fn name(&self) -> &str {
        "remove diacritics"
    }

    fn apply(&self, input: &str) -> String {
        input
            .chars()
            .map(|ch| match ch {
                'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => "a".into(),
                'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Ā' | 'Ă' | 'Ą' => "A".into(),
                'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => "c".into(),
                'Ç' | 'Ć' | 'Č' | 'Ĉ' | 'Ċ' => "C".into(),
                'ď' | 'đ' => "d".into(),
                'Ď' | 'Đ' => "D".into(),
                'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e".into(),
                'É' | 'È' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => "E".into(),
                'ģ' | 'ğ' | 'ĝ' | 'ġ' => "g".into(),
                'Ģ' | 'Ğ' | 'Ĝ' | 'Ġ' => "G".into(),
                'í' | 'ì' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i".into(),
                'Í' | 'Ì' | 'Î' | 'Ï' | 'Ī' | 'Į' | 'İ' => "I".into(),
                'ł' | 'ĺ' | 'ľ' | 'ļ' => "l".into(),
                'Ł' | 'Ĺ' | 'Ľ' | 'Ļ' => "L".into(),
                'ñ' | 'ń' | 'ň' | 'ņ' => "n".into(),
                'Ñ' | 'Ń' | 'Ň' | 'Ņ' => "N".into(),
                'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' | 'ő' => "o".into(),
                'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ø' | 'Ō' | 'Ő' => "O".into(),
                'ŕ' | 'ř' | 'ŗ' => "r".into(),
                'Ŕ' | 'Ř' | 'Ŗ' => "R".into(),
                'ś' | 'š' | 'ş' | 'ŝ' => "s".into(),
                'Ś' | 'Š' | 'Ş' | 'Ŝ' => "S".into(),
                'ť' | 'ţ' => "t".into(),
                'Ť' | 'Ţ' => "T".into(),
                'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => "u".into(),
                'Ú' | 'Ù' | 'Û' | 'Ü' | 'Ū' | 'Ů' | 'Ű' | 'Ų' => "U".into(),
                'ý' | 'ÿ' => "y".into(),
                'Ý' | 'Ÿ' => "Y".into(),
                'ź' | 'ž' | 'ż' => "z".into(),
                'Ź' | 'Ž' | 'Ż' => "Z".into(),
                // Ligatures and the sharp s expand rather than losing a letter.
                'æ' => "ae".into(),
                'Æ' => "AE".into(),
                'œ' => "oe".into(),
                'Œ' => "OE".into(),
                'ß' => "ss".into(),
                other => other.to_string(),
            })
            .collect::<Vec<String>>()
            .concat()
    }
}

/// Transliterate whole non-Latin scripts to Latin (#72) — a different job from
/// [`RemoveDiacritics`], which only strips accents off Latin letters. This maps
/// another alphabet onto Latin (`Пётр` -> `Pyotr`, `Ελλάδα` -> `Ellada`).
///
/// To-Latin only; the reverse (Latin -> Cyrillic) is guesswork and out of scope.
/// Per-script tables keep it lossy-but-documented and make adding a script a
/// data-only change: add a `<script>_to_latin` function and chain it in `apply`.
/// Uppercase letters map to a capitalised Latin form (`Ж` -> `Zh`), lowercase to
/// lowercase (`ж` -> `zh`); non-covered characters (incl. Latin) pass through.
#[derive(Debug, Clone, Copy, Default)]
pub struct Transliterate;

impl TransformStep for Transliterate {
    fn name(&self) -> &str {
        "transliterate to Latin"
    }

    fn apply(&self, input: &str) -> String {
        input
            .chars()
            .map(|ch| {
                cyrillic_to_latin(ch)
                    .or_else(|| greek_to_latin(ch))
                    .map(str::to_string)
                    .unwrap_or_else(|| ch.to_string())
            })
            .collect()
    }
}

/// Russian Cyrillic -> Latin, a BGN/PCGN-style romanization (`ж`->`zh`,
/// `х`->`kh`, `ц`->`ts`, `щ`->`shch`; the hard/soft signs `ъ`/`ь` drop). Returns
/// `None` for non-Cyrillic so the caller can try the next script / pass through.
fn cyrillic_to_latin(ch: char) -> Option<&'static str> {
    Some(match ch {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'д' => "d",
        'е' => "e",
        'ё' => "yo",
        'ж' => "zh",
        'з' => "z",
        'и' => "i",
        'й' => "y",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "kh",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "shch",
        'ъ' => "",
        'ы' => "y",
        'ь' => "",
        'э' => "e",
        'ю' => "yu",
        'я' => "ya",
        'А' => "A",
        'Б' => "B",
        'В' => "V",
        'Г' => "G",
        'Д' => "D",
        'Е' => "E",
        'Ё' => "Yo",
        'Ж' => "Zh",
        'З' => "Z",
        'И' => "I",
        'Й' => "Y",
        'К' => "K",
        'Л' => "L",
        'М' => "M",
        'Н' => "N",
        'О' => "O",
        'П' => "P",
        'Р' => "R",
        'С' => "S",
        'Т' => "T",
        'У' => "U",
        'Ф' => "F",
        'Х' => "Kh",
        'Ц' => "Ts",
        'Ч' => "Ch",
        'Ш' => "Sh",
        'Щ' => "Shch",
        'Ъ' => "",
        'Ы' => "Y",
        'Ь' => "",
        'Э' => "E",
        'Ю' => "Yu",
        'Я' => "Ya",
        _ => return None,
    })
}

/// Latin -> Russian Cyrillic (#137), the reverse of [`Transliterate`] for tags
/// that arrived already romanized.
///
/// Reversing a romanization is guesswork, and this step is built to be wrong as
/// rarely as possible rather than to be clever:
///
/// * **Longest match first.** `shch` is щ before `sh` gets a chance at ш, `ts` is
///   ц before `t` is т. Digraphs are exactly where a naive per-letter reverse
///   falls apart.
/// * **A word is all-or-nothing.** If any letter in a word has no mapping, the
///   whole word is left in Latin. `q`, `w`, `x` and a bare `c`, `h` or `j` never
///   come out of the forward table, so a word containing one was never Cyrillic
///   -- `Jazz` and `The` stay themselves instead of becoming `Jазз` and `Тхе`.
///   Mixed-script mangling is the failure people actually notice.
/// * **And the decision is made once for the whole value** (#258, #259). Asking
///   only whether a word *could* be read back converts most short English words:
///   `desert rain` became `десерт раин`. Asking each word whether it *looks*
///   romanized still converts `bush`, because `sh` is both an English digraph
///   and the romanization of ш. So the value is judged as a whole — see
///   [`value_looks_romanized`] — and every word converts or none does. Mixed
///   script is the failure people notice, and it cannot be ruled out one word at
///   a time.
/// * The cost is stated rather than hidden: a value mixing the languages, like
///   `Zhuk remix`, is left whole instead of half-converted, and a romanized value
///   with no trace to recognise it by (`dom`, `Kino`) is left alone.
///   Under-converting is recoverable by hand; mangling a library is not.
/// * **What the forward direction threw away stays thrown away.** `ъ` and `ь`
///   romanize to nothing, so `Ильич` -> `Ilich` -> `Илич`; `й` and `ы` both
///   romanize to `y`, which comes back as `й`. A round trip is not the identity
///   and cannot be made one.
///
/// Digits, punctuation and text already in Cyrillic pass through without
/// blocking the word around them.
#[derive(Debug, Clone, Copy, Default)]
pub struct Untransliterate;

impl TransformStep for Untransliterate {
    fn name(&self) -> &str {
        "transliterate to Cyrillic"
    }

    fn apply(&self, input: &str) -> String {
        if !value_looks_romanized(input) {
            return input.to_string();
        }
        map_words(input, |word| {
            latin_to_cyrillic_word(word).unwrap_or_else(|| word.to_string())
        })
    }
}

/// Whether a whole value is worth reading back as Cyrillic (#259).
///
/// Two conditions, and both are about the value rather than the word:
///
/// * **Nothing in it is provably not Cyrillic.** One word carrying `q`, `w`,
///   `x` or a bare `c`, `h`, `j` — `music`, `house`, `remix` — says this text is
///   Latin that was always Latin, and the words around it are its neighbours,
///   not romanizations that happen to sit nearby.
/// * **Something in it looks romanized.** At least one word carries a trace the
///   forward direction leaves for a sound Latin has no letter for.
///
/// Deciding once per value is the point. Deciding per word is what produced
/// `la буш - music from the temple of house`: `bush` reads back as `буш` and
/// there is nothing in that word alone to say it should not. The cost is a value
/// mixing the two languages — `Zhuk remix` — which is now left whole rather than
/// half-converted; that is the same answer, chosen deliberately.
fn value_looks_romanized(input: &str) -> bool {
    let mut any_marker = false;
    let mut word = String::new();
    let mut ok = true;
    let check = |word: &str, any_marker: &mut bool, ok: &mut bool| {
        if word.is_empty() {
            return;
        }
        if latin_to_cyrillic_word(word).is_none() {
            *ok = false;
        }
        if looks_romanized(word) {
            *any_marker = true;
        }
    };
    for ch in input.chars() {
        if ch.is_alphanumeric() || ch == '\'' {
            word.push(ch);
        } else {
            check(&word, &mut any_marker, &mut ok);
            word.clear();
        }
    }
    check(&word, &mut any_marker, &mut ok);
    ok && any_marker
}

/// Sequences the forward romanization produces for sounds Latin has no single
/// letter for. A word carrying one plausibly came from Cyrillic; a word carrying
/// none is indistinguishable from ordinary Latin text and is left alone (#258).
const ROMANIZATION_MARKERS: &[&str] = &[
    "shch", "zh", "kh", "ts", "ch", "sh", "yu", "ya", "yo", "iy", "yy",
];

fn looks_romanized(word: &str) -> bool {
    let lower = word.to_lowercase();
    ROMANIZATION_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

/// Latin runs, longest first — order is the whole algorithm, so this stays one
/// list rather than being split by length.
const LATIN_TO_CYRILLIC: &[(&str, char)] = &[
    ("shch", 'щ'),
    ("yo", 'ё'),
    ("yu", 'ю'),
    ("ya", 'я'),
    ("zh", 'ж'),
    ("kh", 'х'),
    ("ts", 'ц'),
    ("ch", 'ч'),
    ("sh", 'ш'),
    ("a", 'а'),
    ("b", 'б'),
    ("v", 'в'),
    ("g", 'г'),
    ("d", 'д'),
    ("e", 'е'),
    ("z", 'з'),
    ("i", 'и'),
    ("y", 'й'),
    ("k", 'к'),
    ("l", 'л'),
    ("m", 'м'),
    ("n", 'н'),
    ("o", 'о'),
    ("p", 'п'),
    ("r", 'р'),
    ("s", 'с'),
    ("t", 'т'),
    ("u", 'у'),
    ("f", 'ф'),
];

/// One word Latin -> Cyrillic, or `None` if any Latin letter in it has no
/// mapping — the caller then keeps the word as it was.
fn latin_to_cyrillic_word(word: &str) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();
    let lower: String = word.to_lowercase();
    // Mapping walks the lowercased form, so the two must agree position for
    // position; a letter that lowercases to several chars (`İ`) would break that
    // and is not something this step claims to handle.
    if lower.chars().count() != chars.len() {
        return None;
    }
    let lower: Vec<char> = lower.chars().collect();

    let mut out = String::with_capacity(word.len());
    let mut at = 0;
    while at < chars.len() {
        if !chars[at].is_ascii_alphabetic() {
            out.push(chars[at]); // digits, apostrophes, Cyrillic already there
            at += 1;
            continue;
        }
        let matched = LATIN_TO_CYRILLIC.iter().find(|(latin, _)| {
            let run = &lower[at..chars.len().min(at + latin.chars().count())];
            run.iter().copied().eq(latin.chars())
        })?;
        let (latin, cyrillic) = *matched;
        // The run's own case decides the result's, the way `Ж` -> `Zh` does in
        // the forward direction.
        if chars[at].is_uppercase() {
            out.extend(cyrillic.to_uppercase());
        } else {
            out.push(cyrillic);
        }
        at += latin.chars().count();
    }
    Some(out)
}

/// Modern Greek -> Latin, a simple per-letter romanization (BGN/PCGN-style:
/// `β`->`v`, `η`->`i`, `θ`->`th`, `χ`->`ch`, `ψ`->`ps`); accented vowels fold to
/// their base letter. No digraph context rules (`μπ`->`b` etc.) — kept per-letter
/// on purpose. Returns `None` for non-Greek.
fn greek_to_latin(ch: char) -> Option<&'static str> {
    Some(match ch {
        'α' | 'ά' => "a",
        'β' => "v",
        'γ' => "g",
        'δ' => "d",
        'ε' | 'έ' => "e",
        'ζ' => "z",
        'η' | 'ή' => "i",
        'θ' => "th",
        'ι' | 'ί' | 'ϊ' | 'ΐ' => "i",
        'κ' => "k",
        'λ' => "l",
        'μ' => "m",
        'ν' => "n",
        'ξ' => "x",
        'ο' | 'ό' => "o",
        'π' => "p",
        'ρ' => "r",
        'σ' | 'ς' => "s",
        'τ' => "t",
        'υ' | 'ύ' | 'ϋ' | 'ΰ' => "y",
        'φ' => "f",
        'χ' => "ch",
        'ψ' => "ps",
        'ω' | 'ώ' => "o",
        'Α' | 'Ά' => "A",
        'Β' => "V",
        'Γ' => "G",
        'Δ' => "D",
        'Ε' | 'Έ' => "E",
        'Ζ' => "Z",
        'Η' | 'Ή' => "I",
        'Θ' => "Th",
        'Ι' | 'Ί' | 'Ϊ' => "I",
        'Κ' => "K",
        'Λ' => "L",
        'Μ' => "M",
        'Ν' => "N",
        'Ξ' => "X",
        'Ο' | 'Ό' => "O",
        'Π' => "P",
        'Ρ' => "R",
        'Σ' => "S",
        'Τ' => "T",
        'Υ' | 'Ύ' | 'Ϋ' => "Y",
        'Φ' => "F",
        'Χ' => "Ch",
        'Ψ' => "Ps",
        'Ω' | 'Ώ' => "O",
        _ => return None,
    })
}

/// Apply `f` to each run of word characters, leaving separators untouched.
fn map_words(input: &str, mut f: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(input.len());
    let mut word = String::new();
    for ch in input.chars() {
        if ch.is_alphanumeric() || ch == '\'' {
            word.push(ch);
        } else {
            if !word.is_empty() {
                out.push_str(&f(&word));
                word.clear();
            }
            out.push(ch);
        }
    }
    if !word.is_empty() {
        out.push_str(&f(&word));
    }
    out
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

#[derive(Debug, Error)]
pub enum TransformError {
    #[error("the search pattern is empty")]
    EmptyPattern,
    #[error("invalid regular expression: {0}")]
    BadPattern(String),
}

/// Target notation for [`KeyNotation`] (#55).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStyle {
    /// Camelot wheel code, e.g. `8A` (A minor), `8B` (C major) — what harmonic
    /// mixing uses.
    Camelot,
    /// Open Key code, e.g. `1m` / `1d`.
    OpenKey,
    /// Compact musical name, e.g. `Am`, `C`, `F#`.
    Musical,
}

/// Camelot number for each pitch class (0 = C … 11 = B) in the major (B side)
/// and minor (A side) rings of the wheel. Relative major/minor share a number
/// (A minor = C major = 8), which is exactly why the wheel works.
const MAJOR_CAMELOT: [u8; 12] = [8, 3, 10, 5, 12, 7, 2, 9, 4, 11, 6, 1];
const MINOR_CAMELOT: [u8; 12] = [5, 12, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10];

/// Preferred compact spelling per pitch class for `Musical` output (the flat
/// spelling most tag data and DJ software use).
const MUSICAL_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

/// A parsed musical key: pitch class (0 = C … 11 = B) and whether it's minor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    pitch: u8,
    minor: bool,
}

/// Convert a musical key between notations (#55): musical ↔ Camelot ↔ Open Key.
/// A [`TransformStep`], so it composes into a chain like the other steps and can
/// batch-convert a whole library's Key field. An unrecognised value is left
/// untouched — the field may legitimately hold something we don't model.
pub struct KeyNotation {
    style: KeyStyle,
}

impl KeyNotation {
    pub fn new(style: KeyStyle) -> Self {
        Self { style }
    }
}

impl TransformStep for KeyNotation {
    fn name(&self) -> &str {
        match self.style {
            KeyStyle::Camelot => "key → Camelot",
            KeyStyle::OpenKey => "key → Open Key",
            KeyStyle::Musical => "key → musical",
        }
    }

    fn apply(&self, input: &str) -> String {
        match parse_key(input) {
            Some(key) => format_key(key, self.style),
            None => input.to_string(),
        }
    }
}

/// Parse a key in musical (`Am`, `F# minor`, `Db`), Camelot (`8A`) or Open Key
/// (`1m`) notation into a [`Key`]. Returns `None` for anything unrecognised.
fn parse_key(input: &str) -> Option<Key> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    // A leading digit means a Camelot (`8A`) or Open Key (`1m`) code.
    if trimmed.as_bytes()[0].is_ascii_digit() {
        return parse_wheel_code(trimmed);
    }
    parse_musical(trimmed)
}

/// Parse `<1-12><A|B>` (Camelot) or `<1-12><m|d>` (Open Key).
fn parse_wheel_code(input: &str) -> Option<Key> {
    let last = input.chars().last()?;
    let number: u8 = input[..input.len() - last.len_utf8()].trim().parse().ok()?;
    if !(1..=12).contains(&number) {
        return None;
    }
    let minor = match last.to_ascii_uppercase() {
        'A' | 'M' => true,  // Camelot A / Open Key m = minor
        'B' | 'D' => false, // Camelot B / Open Key d = major
        _ => return None,
    };
    let table = if minor {
        &MINOR_CAMELOT
    } else {
        &MAJOR_CAMELOT
    };
    // Camelot 'A'/'B' number IS the wheel number; Open Key 'm'/'d' is offset by
    // 5 from Camelot. Try to read the number as Camelot first, else Open Key.
    let camelot = if matches!(last.to_ascii_uppercase(), 'A' | 'B') {
        number
    } else {
        // Open Key n → Camelot (n + 7) wrapped into 1..=12.
        (number + 6) % 12 + 1
    };
    let pitch = (0u8..12).find(|&p| table[p as usize] == camelot)?;
    Some(Key { pitch, minor })
}

/// Parse a musical key: a note (`A`–`G` with an optional `#`/`b`/`♯`/`♭`) plus
/// an optional mode (`m`/`min`/`minor`/`-` = minor; bare or `maj`/`major` =
/// major).
fn parse_musical(input: &str) -> Option<Key> {
    let mut chars = input.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let base = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut rest = chars.as_str();
    let mut pitch = base as i8;
    match rest.chars().next() {
        Some('#') | Some('♯') | Some('s') => {
            pitch += 1;
            rest = &rest[rest.chars().next().unwrap().len_utf8()..];
        }
        Some('b') | Some('♭') => {
            pitch -= 1;
            rest = &rest[rest.chars().next().unwrap().len_utf8()..];
        }
        _ => {}
    }
    let pitch = pitch.rem_euclid(12) as u8;

    // Drop spaces and dashes so "A minor", "A-min" and "Amin" read alike.
    let mode: String = rest
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_lowercase)
        .collect();
    let minor = if mode.is_empty() || mode.starts_with("maj") {
        false
    } else if mode == "m" || mode.starts_with("min") {
        true
    } else {
        // Unknown trailing text (e.g. "A7", "Ddim") — not a key we model.
        return None;
    };
    Some(Key { pitch, minor })
}

fn format_key(key: Key, style: KeyStyle) -> String {
    match style {
        KeyStyle::Camelot => {
            let table = if key.minor {
                MINOR_CAMELOT
            } else {
                MAJOR_CAMELOT
            };
            format!(
                "{}{}",
                table[key.pitch as usize],
                if key.minor { 'A' } else { 'B' }
            )
        }
        KeyStyle::OpenKey => {
            let table = if key.minor {
                MINOR_CAMELOT
            } else {
                MAJOR_CAMELOT
            };
            // Camelot n → Open Key (n + 5) wrapped into 1..=12.
            let open = (table[key.pitch as usize] + 4) % 12 + 1;
            format!("{}{}", open, if key.minor { 'm' } else { 'd' })
        }
        KeyStyle::Musical => {
            format!(
                "{}{}",
                MUSICAL_NAMES[key.pitch as usize],
                if key.minor { "m" } else { "" }
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Uppercase;

    impl TransformStep for Uppercase {
        fn name(&self) -> &str {
            "uppercase"
        }
        fn apply(&self, input: &str) -> String {
            input.to_uppercase()
        }
    }

    #[test]
    fn chain_applies_steps_in_order() {
        let mut chain = TransformChain::default();
        chain.push(Box::new(Uppercase));
        assert_eq!(chain.apply("tagrex"), "TAGREX");
    }

    #[test]
    fn empty_chain_is_identity() {
        let chain = TransformChain::default();
        assert_eq!(chain.apply("tagrex"), "tagrex");
    }

    #[test]
    fn literal_replace_treats_the_pattern_as_text() {
        let step = Replace::new("_", " ", ReplaceOptions::default()).unwrap();
        assert_eq!(step.apply("desert_rain_live"), "desert rain live");

        // Regex metacharacters are literal here, not syntax.
        let dots = Replace::new(".", "-", ReplaceOptions::default()).unwrap();
        assert_eq!(dots.apply("a.b.c"), "a-b-c");

        // A `$` in the replacement is a dollar sign, not a capture reference.
        let price = Replace::new("cost", "$5", ReplaceOptions::default()).unwrap();
        assert_eq!(price.apply("cost"), "$5");
    }

    #[test]
    fn regex_replace_supports_captures() {
        let step = Replace::new(
            r"^(\d+)\s*-\s*",
            "$1. ",
            ReplaceOptions {
                regex: true,
                ..ReplaceOptions::default()
            },
        )
        .unwrap();
        assert_eq!(step.apply("07 - Desert Rain"), "07. Desert Rain");
    }

    #[test]
    fn whole_word_matching_protects_longer_words() {
        let options = ReplaceOptions {
            whole_word: true,
            ..ReplaceOptions::default()
        };
        let step = Replace::new("Dj", "DJ", options).unwrap();
        assert_eq!(step.apply("dj tiesto"), "DJ tiesto");
        // Without the boundary this would corrupt the country name.
        assert_eq!(step.apply("Djibouti"), "Djibouti");
    }

    #[test]
    fn replace_is_case_insensitive_unless_asked_otherwise() {
        let step = Replace::new("featuring", "feat.", ReplaceOptions::default()).unwrap();
        assert_eq!(step.apply("A FEATURING B"), "A feat. B");

        let strict = Replace::new(
            "featuring",
            "feat.",
            ReplaceOptions {
                case_sensitive: true,
                ..ReplaceOptions::default()
            },
        )
        .unwrap();
        assert_eq!(strict.apply("A FEATURING B"), "A FEATURING B");
    }

    #[test]
    fn a_bad_pattern_is_reported_not_ignored() {
        assert!(matches!(
            Replace::new("", "x", ReplaceOptions::default()),
            Err(TransformError::EmptyPattern)
        ));
        assert!(matches!(
            Replace::new(
                "(unclosed",
                "x",
                ReplaceOptions {
                    regex: true,
                    ..ReplaceOptions::default()
                }
            ),
            Err(TransformError::BadPattern(_))
        ));
    }

    #[test]
    fn title_case_keeps_acronyms_and_roman_numerals() {
        let step = ChangeCase::new(CaseStyle::Title);
        assert_eq!(step.apply("desert rain"), "Desert Rain");
        // The whole reason an exception list exists.
        assert_eq!(step.apply("dj tiesto"), "DJ Tiesto");
        assert_eq!(step.apply("SYMPHONY iii"), "Symphony III");
        assert_eq!(step.apply("a vs b"), "A vs B");
        // Separators and punctuation survive untouched.
        assert_eq!(step.apply("a-b (live)"), "A-B (Live)");
    }

    #[test]
    fn single_letter_roman_numerals_are_left_alone() {
        // `I` is an ordinary word; forcing it uppercase would be worse than the
        // problem the exception list solves.
        let step = ChangeCase::new(CaseStyle::Title);
        assert_eq!(step.apply("i feel it"), "I Feel It");
        assert_eq!(step.apply("what i did"), "What I Did");
    }

    #[test]
    fn sentence_case_capitalises_only_the_first_word() {
        let step = ChangeCase::new(CaseStyle::Sentence);
        assert_eq!(step.apply("DESERT RAIN LIVE"), "Desert rain live");
        // Exceptions still win over the lowercasing.
        assert_eq!(step.apply("play the cd now"), "Play the CD now");
    }

    #[test]
    fn case_exceptions_can_be_replaced_wholesale() {
        let step = ChangeCase::with_exceptions(CaseStyle::Title, vec!["NBG".to_string()]);
        assert_eq!(step.apply("nbg - universal love"), "NBG - Universal Love");
        // Not in the custom list any more.
        assert_eq!(step.apply("dj shadow"), "Dj Shadow");
    }

    #[test]
    fn diacritics_are_stripped_to_base_letters() {
        let step = RemoveDiacritics;
        assert_eq!(step.apply("Björk"), "Bjork");
        assert_eq!(step.apply("Sigur Rós"), "Sigur Ros");
        assert_eq!(
            step.apply("Стас"),
            "Стас",
            "non-latin scripts are left alone"
        );
        // Ligatures and ß expand rather than losing a letter.
        assert_eq!(step.apply("Encyclopædia"), "Encyclopaedia");
        assert_eq!(step.apply("Straße"), "Strasse");
    }

    #[test]
    fn transliterates_cyrillic_to_latin() {
        let step = Transliterate;
        assert_eq!(step.apply("Пётр"), "Pyotr");
        assert_eq!(step.apply("Москва"), "Moskva");
        // The hard/soft signs drop rather than becoming a stray letter.
        assert_eq!(step.apply("Область"), "Oblast");
        // Multi-letter romanizations keep their case ("Ж" -> "Zh", "ж" -> "zh").
        assert_eq!(step.apply("Жук жук"), "Zhuk zhuk");
    }

    #[test]
    fn transliterates_greek_to_latin() {
        let step = Transliterate;
        assert_eq!(step.apply("Ελλάδα"), "Ellada");
        assert_eq!(step.apply("Θεσσαλονίκη"), "Thessaloniki");
    }

    #[test]
    fn transliterate_leaves_latin_untouched() {
        // Unlike RemoveDiacritics, this maps a *different* alphabet — it does not
        // strip accents off Latin letters, so Latin text passes through verbatim.
        let step = Transliterate;
        assert_eq!(step.apply("Björk"), "Björk");
        assert_eq!(step.apply("Sigur Rós - Sæglópur"), "Sigur Rós - Sæglópur");
    }

    #[test]
    fn untransliterates_latin_to_cyrillic() {
        let step = Untransliterate;
        assert_eq!(step.apply("Pyotr"), "Пётр");
        assert_eq!(step.apply("borshch"), "борщ");
        // Digraph before letter: `ts` is ц, not т + с.
        assert_eq!(step.apply("Tsoy"), "Цой");
        // ...and a word with nothing to recognise it by is left alone (#258),
        // even though every one of its letters could be read back.
        assert_eq!(step.apply("Kino"), "Kino");
    }

    #[test]
    fn untransliterate_leaves_text_that_does_not_look_romanized() {
        // The reported case (#258): pointed at an English title, the per-word
        // guard alone converted whatever happened to be readable and left the
        // rest, which is mixed-script mangling one level up.
        let step = Untransliterate;
        assert_eq!(step.apply("desert rain"), "desert rain");
        assert_eq!(
            step.apply("music from the temple of house"),
            "music from the temple of house"
        );
        assert_eq!(step.apply("various"), "various");
        // What the markers are for: a value that carries one still converts.
        assert_eq!(step.apply("Ilich"), "Илич");
        assert_eq!(step.apply("Zhuk"), "Жук");
        assert_eq!(step.apply("ulitsa"), "улица");
    }

    #[test]
    fn untransliterate_decides_once_for_the_whole_value() {
        // #259, the reported case: `bush` reads back as `буш` and nothing in
        // that word alone says it should not — but `music` and `house` in the
        // same value were never Cyrillic, which settles it for all of them.
        let step = Untransliterate;
        assert_eq!(
            step.apply("la_bush_-_music_from_the_temple_of_house"),
            "la_bush_-_music_from_the_temple_of_house"
        );
        // With nothing in the value to contradict it, every word converts —
        // including the ones with no trace of their own, which is what makes a
        // real romanization come back whole instead of half-Latin.
        assert_eq!(step.apply("Masha i Medved"), "Маша и Медвед");
        // And the cost, deliberately: a value mixing the two languages is left
        // whole rather than half-converted.
        assert_eq!(step.apply("Zhuk remix"), "Zhuk remix");
    }

    #[test]
    fn untransliterate_leaves_a_word_it_cannot_map_alone() {
        // A word containing a letter the forward direction never produces was
        // never Cyrillic. Half-converting it is the visible failure, so the word
        // is kept whole rather than mangled into mixed script.
        let step = Untransliterate;
        assert_eq!(step.apply("Jazz"), "Jazz");
        assert_eq!(step.apply("The Quick Fox"), "The Quick Fox");
        // And it takes the value with it (#259): one word that was never
        // Cyrillic says the whole value is Latin, so its neighbours are left
        // alone too rather than half the line coming back in another script.
        assert_eq!(step.apply("Jazz na ulitse"), "Jazz na ulitse");
    }

    #[test]
    fn untransliterate_keeps_the_case_of_the_run_it_matched() {
        // Each matched run carries its own case, so an all-caps word stays all
        // caps and a capitalised one keeps just its initial -- a four-letter run
        // like `Shch` collapsing to one Cyrillic letter doesn't change that.
        let step = Untransliterate;
        assert_eq!(step.apply("SHCHI"), "ЩИ");
        assert_eq!(step.apply("Shchi"), "Щи");
        assert_eq!(step.apply("Zhuk"), "Жук");
    }

    #[test]
    fn untransliterate_does_not_claim_to_round_trip() {
        // What the forward direction discards is gone: the soft sign romanizes to
        // nothing, and й/ы share `y`. Documented rather than papered over.
        let there = Transliterate;
        let back = Untransliterate;
        assert_eq!(there.apply("Ильич"), "Ilich");
        assert_eq!(back.apply("Ilich"), "Илич");
        assert_eq!(back.apply(&there.apply("Пётр")), "Пётр");
    }

    #[test]
    fn untransliterate_passes_digits_and_punctuation_through() {
        let step = Untransliterate;
        // Digits and the hyphen neither block the word nor come back changed.
        assert_eq!(step.apply("dozhd-2"), "дожд-2");
        assert_eq!(step.apply("Пётр"), "Пётр");
    }

    #[test]
    fn transliterate_then_diacritics_composes() {
        // Transliterate first, then any leftover Latin accents are stripped — the
        // two steps cover different alphabets and chain cleanly.
        let mut chain = TransformChain::default();
        chain.push(Box::new(Transliterate));
        chain.push(Box::new(RemoveDiacritics));
        assert_eq!(chain.apply("Björk — Пётр"), "Bjork — Pyotr");
    }

    #[test]
    fn a_realistic_cleanup_chain_composes() {
        // Underscores to spaces, then title case with the acronym repair.
        let mut chain = TransformChain::default();
        chain.push(Box::new(
            Replace::new("_", " ", ReplaceOptions::default()).unwrap(),
        ));
        chain.push(Box::new(ChangeCase::new(CaseStyle::Title)));
        chain.push(Box::new(RemoveDiacritics));
        assert_eq!(
            chain.apply("dj_kicks_björk_vol_iii"),
            "DJ Kicks Bjork Vol III"
        );
    }

    #[test]
    fn key_to_camelot_covers_the_wheel() {
        let camelot = KeyNotation::new(KeyStyle::Camelot);
        // Relative major/minor share a number (the whole point of the wheel).
        assert_eq!(camelot.apply("Am"), "8A");
        assert_eq!(camelot.apply("C"), "8B");
        assert_eq!(camelot.apply("Cm"), "5A");
        assert_eq!(camelot.apply("Eb"), "5B");
        // Sharps, flats, unicode accidentals, and mode spellings all parse.
        assert_eq!(camelot.apply("F#"), "2B");
        assert_eq!(camelot.apply("Gb"), "2B"); // enharmonic with F#
        assert_eq!(camelot.apply("F♯ minor"), "11A");
        assert_eq!(camelot.apply("bb min"), "3A"); // Bb minor, lower-case
        assert_eq!(camelot.apply("A major"), "11B");
    }

    #[test]
    fn key_converts_between_wheel_and_musical() {
        // Camelot in, musical out.
        let musical = KeyNotation::new(KeyStyle::Musical);
        assert_eq!(musical.apply("8A"), "Am");
        assert_eq!(musical.apply("8B"), "C");
        assert_eq!(musical.apply("2B"), "F#");
        // Open Key in, Camelot out (8A == 1m).
        let camelot = KeyNotation::new(KeyStyle::Camelot);
        assert_eq!(camelot.apply("1m"), "8A");
        assert_eq!(camelot.apply("1d"), "8B");
        // Musical in, Open Key out.
        let open = KeyNotation::new(KeyStyle::OpenKey);
        assert_eq!(open.apply("Am"), "1m");
        assert_eq!(open.apply("C"), "1d");
    }

    #[test]
    fn key_leaves_unrecognized_values_untouched() {
        let camelot = KeyNotation::new(KeyStyle::Camelot);
        assert_eq!(camelot.apply(""), "");
        assert_eq!(camelot.apply("Ddim"), "Ddim"); // not a plain major/minor key
        assert_eq!(camelot.apply("not a key"), "not a key");
        assert_eq!(camelot.apply("13A"), "13A"); // out of the 1..=12 range
                                                 // Already-Camelot input is idempotent.
        assert_eq!(camelot.apply("8A"), "8A");
    }
}
