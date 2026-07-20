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
    /// Difference between the two listed lengths in seconds, when both are
    /// known — so a caller can flag "the title matches but the length doesn't".
    pub duration_delta: Option<u64>,
}

/// Length agreement within this many seconds counts as confirmation: encoder
/// delay/padding and sleeve rounding both live well inside it.
const DURATION_EXACT_SECS: u64 = 2;

/// Past this the lengths are evidence of a different recording (an edit, a
/// different pressing, a differently-split rip of a continuous mix).
const DURATION_CONFLICT_SECS: u64 = 30;

/// How much of the final score length agreement may swing. Deliberately a
/// minority share: provider durations are hand-transcribed and often wrong or
/// absent, so they refine a ranking but must never overturn the title (#64).
const DURATION_WEIGHT: f32 = 0.3;

/// Default tolerance for [`align_by_duration_sequence`].
pub const DURATION_SEQUENCE_TOLERANCE_SECS: u64 = 5;

/// 1.0 when the lengths agree, decaying to 0.0 as they diverge.
fn duration_score(delta: u64) -> f32 {
    if delta <= DURATION_EXACT_SECS {
        1.0
    } else if delta >= DURATION_CONFLICT_SECS {
        0.0
    } else {
        let span = (DURATION_CONFLICT_SECS - DURATION_EXACT_SECS) as f32;
        1.0 - (delta - DURATION_EXACT_SECS) as f32 / span
    }
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

    let (title, reason) = title_score(local.title, candidate.title);
    // Accept/reject is decided on the title alone; an exact match stands on its
    // own, a fuzzy one has to clear the bar.
    if reason == MatchReason::Fuzzy && title < options.strictness {
        return None;
    }

    // Length only refines the ranking from here — it never rejects, because
    // provider durations disagree with real files too often to be trusted that
    // far (#64). The opt-in gate above is the escape hatch for callers that do
    // want a hard filter.
    let duration_delta = match (local.duration_secs, candidate.duration_secs) {
        (Some(a), Some(b)) => Some(a.abs_diff(b)),
        _ => None,
    };
    let score = match duration_delta {
        Some(delta) => title * (1.0 - DURATION_WEIGHT + DURATION_WEIGHT * duration_score(delta)),
        None => title,
    };

    Some(Match {
        score,
        reason,
        duration_delta,
    })
}

/// Align two ordered track lists using **only** their lengths.
///
/// For a folder of `track01.mp3`-style files there is no usable title to match
/// on, but the ordered vector of durations is effectively a fingerprint of the
/// release: even with a couple of seconds of noise per track, a full sequence
/// lines up unambiguously. Order is preserved and both sides may have gaps
/// (extra local files, tracks missing from the folder), so this is a classic
/// sequence alignment rather than a per-track best guess.
///
/// Returns, for each local track, the index of the candidate it lines up with.
pub fn align_by_duration_sequence(
    locals: &[TrackRef<'_>],
    candidates: &[TrackRef<'_>],
    tolerance_secs: u64,
) -> Vec<Option<usize>> {
    let (rows, columns) = (locals.len(), candidates.len());
    // Cell = (pairs matched so far, total seconds of disagreement). More pairs
    // always wins; ties go to the tighter fit.
    let mut best = vec![vec![(0u32, 0u64); columns + 1]; rows + 1];

    let pairing = |i: usize, j: usize| -> Option<u64> {
        match (locals[i].duration_secs, candidates[j].duration_secs) {
            (Some(a), Some(b)) if a.abs_diff(b) <= tolerance_secs => Some(a.abs_diff(b)),
            _ => None,
        }
    };

    for i in 1..=rows {
        for j in 1..=columns {
            let mut cell = better(best[i - 1][j], best[i][j - 1]);
            if let Some(delta) = pairing(i - 1, j - 1) {
                let previous = best[i - 1][j - 1];
                cell = better(cell, (previous.0 + 1, previous.1 + delta));
            }
            best[i][j] = cell;
        }
    }

    let mut result = vec![None; rows];
    let (mut i, mut j) = (rows, columns);
    while i > 0 && j > 0 {
        let current = best[i][j];
        let paired = pairing(i - 1, j - 1).map(|delta| {
            let previous = best[i - 1][j - 1];
            (previous.0 + 1, previous.1 + delta)
        });
        if paired == Some(current) {
            result[i - 1] = Some(j - 1);
            i -= 1;
            j -= 1;
        } else if best[i - 1][j] == current {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    result
}

fn better(a: (u32, u64), b: (u32, u64)) -> (u32, u64) {
    if a.0 > b.0 || (a.0 == b.0 && a.1 <= b.1) {
        a
    } else {
        b
    }
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

    fn timed<'a>(title: &'a str, secs: u64) -> TrackRef<'a> {
        TrackRef {
            title,
            artist: None,
            duration_secs: Some(secs),
        }
    }

    #[test]
    fn length_agreement_refines_ranking_but_never_rejects() {
        let options = MatchOptions::default();

        // Same title, lengths agree -> full confidence.
        let agree = match_track(
            &timed("Desert Rain", 278),
            &timed("Desert Rain", 279),
            &options,
        )
        .expect("must match");
        assert_eq!(agree.reason, MatchReason::Exact);
        assert_eq!(agree.score, 1.0);
        assert_eq!(agree.duration_delta, Some(1));

        // Same title, lengths clearly disagree: still a match (the title is what
        // decides), but ranked below, and the delta is exposed so the UI can
        // warn about it.
        let disagree = match_track(
            &timed("Desert Rain", 278),
            &timed("Desert Rain", 500),
            &options,
        )
        .expect("a length mismatch must not reject a title match");
        assert_eq!(disagree.reason, MatchReason::Exact);
        assert!(disagree.score < agree.score);
        assert_eq!(disagree.duration_delta, Some(222));
    }

    #[test]
    fn length_breaks_a_tie_between_identically_titled_candidates() {
        let options = MatchOptions::default();
        let local = timed("Universal Love", 368);
        // Two pressings of the same title; only the length tells them apart.
        let candidates = [timed("Universal Love", 210), timed("Universal Love", 367)];
        let (index, _) = best_match(&local, &candidates, &options).expect("must match");
        assert_eq!(index, 1);
    }

    #[test]
    fn duration_sequence_aligns_untitled_files() {
        // No usable titles at all — the ordered lengths are the only signal.
        let locals = [
            timed("track01", 279),
            timed("track02", 141),
            timed("track03", 322),
        ];
        let candidates = [
            timed("Desert Rain", 278),
            timed("Radio", 142),
            timed("Voodoo Rhythm", 321),
        ];
        assert_eq!(
            align_by_duration_sequence(&locals, &candidates, DURATION_SEQUENCE_TOLERANCE_SECS),
            vec![Some(0), Some(1), Some(2)]
        );
    }

    #[test]
    fn duration_sequence_tolerates_gaps_on_both_sides() {
        // An extra local file that isn't on the release at all.
        let locals = [timed("a", 278), timed("stray", 999), timed("b", 142)];
        let candidates = [timed("x", 278), timed("y", 142)];
        assert_eq!(
            align_by_duration_sequence(&locals, &candidates, DURATION_SEQUENCE_TOLERANCE_SECS),
            vec![Some(0), None, Some(1)]
        );

        // A folder holding only part of the release keeps the right partners.
        let partial = [timed("only", 142)];
        assert_eq!(
            align_by_duration_sequence(&partial, &candidates, DURATION_SEQUENCE_TOLERANCE_SECS),
            vec![Some(1)]
        );
    }

    #[test]
    fn duration_sequence_preserves_order_and_ignores_unknown_lengths() {
        // Order is preserved: a later file cannot claim an earlier track.
        let locals = [timed("a", 142), timed("b", 278)];
        let candidates = [timed("x", 278), timed("y", 142)];
        let aligned =
            align_by_duration_sequence(&locals, &candidates, DURATION_SEQUENCE_TOLERANCE_SECS);
        assert!(
            aligned.iter().flatten().count() <= 1,
            "must not cross-match out of order"
        );

        // A track with no known length simply doesn't pair.
        let unknown = [track("no length", None)];
        assert_eq!(
            align_by_duration_sequence(&unknown, &candidates, DURATION_SEQUENCE_TOLERANCE_SECS),
            vec![None]
        );
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
