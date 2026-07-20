//! Composable text transforms.
//!
//! Used by masks, manual edits and provider post-processing. Deliberately a
//! chain of steps: Mp3tag-style "actions"/scripting later becomes
//! *serialization of chains into saved presets*, not a new subsystem
//! (architecture.md, "Deferred").
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
}
