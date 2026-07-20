//! Matching a local track against provider candidates.
//!
//! Providers hand back results in whatever order their API felt like; that
//! order is not evidence. This module decides how well a candidate actually
//! matches a local file, so a release import can align tracks by *content*
//! instead of by position — the failure mode being a whole album tagged one
//! title out of step.
//!
//! Comparison is progressive: titles are normalized in increasing strength
//! (case, punctuation, leading articles, then throwaway attributes like
//! "Original Mix") and an exact hit at any level beats fuzzy scoring. Only if
//! nothing matches exactly do we fall back to a normalized Levenshtein
//! similarity, gated by a configurable strictness threshold.
//!
//! Deliberately free of provider, network and GUI concerns so it can be tested
//! against plain strings.

/// Attributes that carry no identity — stripping them lets "Desert Rain" match
/// "Desert Rain (Original Mix)". Remixer credits are **not** listed here: a
/// remix is a different recording, and collapsing it onto the original is
/// exactly the kind of silent mistagging this module exists to prevent.
const NOISE_ATTRIBUTES: &[&str] = &[
    "original mix",
    "original version",
    "album version",
    "single version",
    "radio edit",
    "radio version",
    "extended mix",
    "extended version",
    "remastered",
    "remaster",
    "explicit",
    "clean",
    "bonus track",
];

/// How strict a match has to be to count.
#[derive(Debug, Clone)]
pub struct MatchOptions {
    /// Minimum title similarity (0.0..=1.0) for a fuzzy match to be accepted.
    pub strictness: f32,
    /// Require the artists to corroborate the title match, when both sides
    /// actually have an artist.
    pub require_artist: bool,
    /// Reject candidates whose length differs by more than this many seconds,
    /// when both sides know their duration. `None` disables the check.
    pub max_duration_diff_secs: Option<u64>,
}

impl Default for MatchOptions {
    fn default() -> Self {
        Self {
            strictness: 0.7,
            require_artist: false,
            max_duration_diff_secs: None,
        }
    }
}

/// The minimum a track needs to expose to be matched, local or remote.
#[derive(Debug, Clone, Default)]
pub struct TrackRef<'a> {
    pub title: &'a str,
    pub artist: Option<&'a str>,
    pub duration_secs: Option<u64>,
}

/// Why a candidate matched — worth surfacing so the user can tell a confident
/// match from a lucky one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchReason {
    /// Identical after normalization.
    Exact,
    /// Accepted on similarity alone.
    Fuzzy,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Match {
    /// 0.0..=1.0, higher is better.
    pub score: f32,
    pub reason: MatchReason,
}

/// Score `candidate` against `local`, or `None` if it fails a gate or falls
/// below the strictness threshold.
pub fn match_track(
    local: &TrackRef<'_>,
    candidate: &TrackRef<'_>,
    options: &MatchOptions,
) -> Option<Match> {
    if let (Some(max), Some(a), Some(b)) = (
        options.max_duration_diff_secs,
        local.duration_secs,
        candidate.duration_secs,
    ) {
        if a.abs_diff(b) > max {
            return None;
        }
    }

    if options.require_artist {
        if let (Some(local_artist), Some(candidate_artist)) = (local.artist, candidate.artist) {
            if !artists_match(local_artist, candidate_artist) {
                return None;
            }
        }
    }

    let (score, reason) = title_score(local.title, candidate.title);
    // An exact title match stands on its own; a fuzzy one has to clear the bar.
    if reason == MatchReason::Fuzzy && score < options.strictness {
        return None;
    }
    Some(Match { score, reason })
}

/// The best-scoring candidate for `local`, with its index.
pub fn best_match(
    local: &TrackRef<'_>,
    candidates: &[TrackRef<'_>],
    options: &MatchOptions,
) -> Option<(usize, Match)> {
    candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            match_track(local, candidate, options).map(|found| (index, found))
        })
        .max_by(|(_, a), (_, b)| a.score.total_cmp(&b.score))
}

/// Align local tracks to candidates one-to-one.
///
/// Greedy best-first over every pair, so the most confident matches claim their
/// partner before weaker ones do — a locally-best guess can't steal a candidate
/// that another file matches far better. Returns, for each local track, the
/// index of its candidate (or `None` when nothing cleared the threshold).
pub fn align(
    locals: &[TrackRef<'_>],
    candidates: &[TrackRef<'_>],
    options: &MatchOptions,
) -> Vec<Option<usize>> {
    let mut pairs: Vec<(usize, usize, f32)> = Vec::new();
    for (local_index, local) in locals.iter().enumerate() {
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            if let Some(found) = match_track(local, candidate, options) {
                pairs.push((local_index, candidate_index, found.score));
            }
        }
    }
    // Highest score first; ties broken by position so the result is stable.
    pairs.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));

    let mut result = vec![None; locals.len()];
    let mut taken = vec![false; candidates.len()];
    for (local_index, candidate_index, _) in pairs {
        if result[local_index].is_none() && !taken[candidate_index] {
            result[local_index] = Some(candidate_index);
            taken[candidate_index] = true;
        }
    }
    result
}

/// Similarity of two free-form strings after the same normalization used for
/// titles, 0.0..=1.0. Useful for ranking search results against a query.
pub fn text_similarity(a: &str, b: &str) -> f32 {
    title_score(a, b).0
}

/// Compare two titles, trying each normalization level for an exact hit before
/// falling back to similarity on the most-normalized forms.
fn title_score(a: &str, b: &str) -> (f32, MatchReason) {
    let a_variants = variants(a);
    let b_variants = variants(b);
    for (left, right) in a_variants.iter().zip(b_variants.iter()) {
        if !left.is_empty() && left == right {
            return (1.0, MatchReason::Exact);
        }
    }
    let best = a_variants
        .iter()
        .zip(b_variants.iter())
        .map(|(left, right)| similarity(left, right))
        .fold(0.0_f32, f32::max);
    (best, MatchReason::Fuzzy)
}

/// Do two artist strings corroborate each other? Deliberately lenient: credits
/// differ constantly between databases ("Wishmountain" vs "Wish Mountain",
/// "A & B" vs "A and B"), and the artist is a supporting signal, not the key.
fn artists_match(a: &str, b: &str) -> bool {
    let left = normalized(a);
    let right = normalized(b);
    if left.is_empty() || right.is_empty() {
        return true;
    }
    left == right
        || left.contains(&right)
        || right.contains(&left)
        || similarity(&left, &right) >= 0.8
}

/// Cumulative normalization levels, gentlest first.
fn variants(value: &str) -> Vec<String> {
    let basic = normalized(value);
    let no_noise = strip_noise(&basic);
    let no_punctuation = strip_punctuation(&no_noise);
    let no_article = strip_leading_article(&no_punctuation);
    vec![basic, no_noise, no_punctuation, no_article]
}

/// Lowercase, collapse whitespace, trim.
fn normalized(value: &str) -> String {
    value
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Drop bracketed groups that only carry a throwaway attribute, plus the same
/// phrases appearing bare. A bracketed group we don't recognize is kept — it is
/// probably a remix credit, which distinguishes recordings.
fn strip_noise(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(open) = rest.find(['(', '[']) {
        let close_char = if rest.as_bytes()[open] == b'(' {
            ')'
        } else {
            ']'
        };
        let Some(close) = rest[open..].find(close_char).map(|index| open + index) else {
            break;
        };
        let inner = &rest[open + 1..close];
        out.push_str(&rest[..open]);
        if !is_noise(inner) {
            out.push_str(&rest[open..=close]);
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);

    let mut cleaned = normalized(&out);
    for noise in NOISE_ATTRIBUTES {
        if let Some(index) = cleaned.find(noise) {
            // Only strip a bare trailing attribute, never text in the middle.
            if index + noise.len() == cleaned.len() {
                cleaned.truncate(index);
            }
        }
    }
    normalized(&cleaned)
}

fn is_noise(inner: &str) -> bool {
    let inner = normalized(inner);
    NOISE_ATTRIBUTES.contains(&inner.as_str())
}

/// Keep alphanumerics and spaces; everything else becomes a space.
fn strip_punctuation(value: &str) -> String {
    let mapped: String = value
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    normalized(&mapped)
}

fn strip_leading_article(value: &str) -> String {
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = value.strip_prefix(article) {
            return rest.to_string();
        }
    }
    value.to_string()
}

/// Levenshtein distance normalized to 0.0..=1.0 similarity.
fn similarity(a: &str, b: &str) -> f32 {
    if a == b {
        return 1.0;
    }
    let left: Vec<char> = a.chars().collect();
    let right: Vec<char> = b.chars().collect();
    let longest = left.len().max(right.len());
    if longest == 0 {
        return 1.0;
    }
    1.0 - (levenshtein(&left, &right) as f32 / longest as f32)
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, left) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, right) in b.iter().enumerate() {
            let cost = usize::from(left != right);
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track<'a>(title: &'a str, artist: Option<&'a str>) -> TrackRef<'a> {
        TrackRef {
            title,
            artist,
            duration_secs: None,
        }
    }

    #[test]
    fn identical_and_case_only_differences_are_exact() {
        let options = MatchOptions::default();
        let found = match_track(
            &track("Desert Rain", None),
            &track("desert rain", None),
            &options,
        )
        .expect("should match");
        assert_eq!(found.reason, MatchReason::Exact);
        assert_eq!(found.score, 1.0);
    }

    #[test]
    fn throwaway_attributes_are_stripped_but_remixes_are_not() {
        let options = MatchOptions::default();

        // "(Original Mix)" carries no identity -> still an exact match.
        let found = match_track(
            &track("Desert Rain", None),
            &track("Desert Rain (Original Mix)", None),
            &options,
        )
        .expect("should match");
        assert_eq!(found.reason, MatchReason::Exact);

        // A remix is a different recording: it must not collapse onto the
        // original, or an import would silently mistag it.
        let remix = match_track(
            &track("Creative Nature", None),
            &track("Creative Nature (Gallione Remix)", None),
            &options,
        );
        assert!(
            remix.is_none_or(|m| m.reason != MatchReason::Exact),
            "a remix must never count as an exact match"
        );
    }

    #[test]
    fn punctuation_and_articles_do_not_block_a_match() {
        let options = MatchOptions::default();
        assert!(match_track(
            &track("Seven Days And One Week", None),
            &track("Seven Days & One Week", None),
            &options
        )
        .is_some());
        assert!(match_track(
            &track("The Innocent", None),
            &track("Innocent", None),
            &options
        )
        .is_some());
    }

    #[test]
    fn unrelated_titles_are_rejected() {
        let options = MatchOptions::default();
        assert!(match_track(
            &track("Desert Rain", None),
            &track("Voodoo Rhythm", None),
            &options
        )
        .is_none());
    }

    #[test]
    fn duration_and_artist_gates_reject_mismatches() {
        let options = MatchOptions {
            max_duration_diff_secs: Some(10),
            require_artist: true,
            ..MatchOptions::default()
        };
        let local = TrackRef {
            title: "Radio",
            artist: Some("Wishmountain"),
            duration_secs: Some(142),
        };

        // Same title, wildly different length -> not the same recording.
        let long = TrackRef {
            title: "Radio",
            artist: Some("Wishmountain"),
            duration_secs: Some(400),
        };
        assert!(match_track(&local, &long, &options).is_none());

        // Spelling variation in the artist still corroborates.
        let spaced = TrackRef {
            title: "Radio",
            artist: Some("Wish Mountain"),
            duration_secs: Some(140),
        };
        assert!(match_track(&local, &spaced, &options).is_some());

        // A genuinely different artist does not.
        let other = TrackRef {
            title: "Radio",
            artist: Some("Gigi D'Agostino"),
            duration_secs: Some(140),
        };
        assert!(match_track(&local, &other, &options).is_none());
    }

    #[test]
    fn align_recovers_a_shuffled_order_one_to_one() {
        let options = MatchOptions::default();
        let locals = [
            track("Desert Rain", None),
            track("Radio", None),
            track("Voodoo Rhythm", None),
        ];
        // Same three tracks, different order and decorated differently.
        let candidates = [
            track("Voodoo Rhythm (Original Mix)", None),
            track("desert rain", None),
            track("Radio", None),
        ];
        assert_eq!(
            align(&locals, &candidates, &options),
            vec![Some(1), Some(2), Some(0)]
        );
    }

    #[test]
    fn align_leaves_unmatched_locals_empty_and_never_reuses_a_candidate() {
        let options = MatchOptions::default();
        let locals = [track("Desert Rain", None), track("Nothing Like It", None)];
        let candidates = [track("Desert Rain", None)];
        let aligned = align(&locals, &candidates, &options);
        assert_eq!(aligned, vec![Some(0), None]);
    }
}
