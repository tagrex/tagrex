//! Property tests of the bidirectional mask grammar (#73, #140): one parsed
//! pattern renders a file name from tags and extracts tags from a file name, so
//! the two directions must agree, and neither may panic on any input.
//! Deterministic: a fixed-seed generator, sized to run in CI.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use tagrex_core::mask::Mask;
use tagrex_core::model::{TagField, TagMap};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const PIECES: &[&str] = &[
    "%artist%",
    "%title%",
    "%album%",
    "%year%",
    "%track%",
    "%track:3%",
    "%disc:1%",
    "%genre%",
    "%skip%",
    "%side%",
    "%filename%",
    "%_bitrate%",
    "%unknown%",
    "%",
    "%%",
    "[",
    "]",
    "[%year% - ]",
    "'",
    "''",
    "'%'",
    " - ",
    "_",
    "/",
    "\\",
    ".",
    "(",
    ")",
    ",",
    "$upper(",
    "$left(%title%,3)",
    "$if(%year%,x,y)",
    "$replace(%artist%, ,_)",
    "$num(%track%,2)",
    "$",
    "$nope(",
    "ü",
    "日本",
    "🎵",
    "\u{0}",
    " ",
    "a",
    "1",
];

const VALUES: &[&str] = &[
    "",
    "x",
    "Kino",
    "Кино",
    "AC/DC",
    "a\\b",
    "What?",
    "  ",
    "1",
    "07",
    "1996",
    "A1",
    "日本 🎵",
    "%artist%",
    "[x]",
    "'q'",
    "$upper(x)",
    ".",
    "..",
    "-",
    " - ",
];

fn random_pattern(rng: &mut Rng) -> String {
    (0..rng.below(8)).map(|_| *rng.pick(PIECES)).collect()
}

fn random_tags(rng: &mut Rng) -> TagMap {
    let fields = [
        TagField::Artist,
        TagField::Title,
        TagField::Album,
        TagField::Year,
        TagField::TrackNumber,
        TagField::DiscNumber,
        TagField::Genre,
    ];
    let mut tags = BTreeMap::new();
    for field in fields {
        if rng.below(3) > 0 {
            tags.insert(field, rng.pick(VALUES).to_string());
        }
    }
    tags
}

/// No input to parse, render or extract panics or runs away.
#[test]
fn no_pattern_tag_set_or_file_name_panics() {
    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    let mut slow = Vec::new();
    for _ in 0..20_000 {
        let pattern = random_pattern(&mut rng);
        let tags = random_tags(&mut rng);
        let name: String = (0..rng.below(6)).map(|_| *rng.pick(VALUES)).collect();
        let started = Instant::now();
        let outcome = std::panic::catch_unwind(|| {
            if let Ok(mask) = Mask::parse(&pattern) {
                let _ = mask.render(&tags);
                let _ = mask.extract(&name);
                let _ = mask.fields();
            }
        });
        assert!(
            outcome.is_ok(),
            "panic: pattern {pattern:?}, tags {tags:?}, name {name:?}"
        );
        if started.elapsed() > Duration::from_millis(200) {
            slow.push(pattern);
        }
    }
    assert!(slow.is_empty(), "slow patterns: {slow:?}");
}

/// A path separator in a tag value never splits the rendered name.
#[test]
fn a_separator_in_a_value_never_reaches_the_rendered_name() {
    let mask = Mask::parse("%artist% - %album% - %title%").unwrap();
    let mut rng = Rng(0x5EED_1234_ABCD_0002);
    for _ in 0..2_000 {
        let mut tags = random_tags(&mut rng);
        for field in [TagField::Artist, TagField::Album, TagField::Title] {
            let value = format!(
                "{}/{}\\{}",
                rng.pick(VALUES),
                rng.pick(VALUES),
                rng.pick(VALUES)
            );
            tags.insert(field, value);
        }
        let rendered = mask.render(&tags).unwrap();
        assert!(
            !rendered.contains(['/', '\\']),
            "{rendered:?} from {tags:?}"
        );
    }
}

/// What a mask renders, the same mask extracts back, when no value holds the
/// text that separates it from the next one.
#[test]
fn render_then_extract_gives_the_tags_back() {
    let separators = [" - ", "_", " (", ") ", ". ", " ~ ", "] ["];
    let fields = [
        ("%artist%", TagField::Artist),
        ("%title%", TagField::Title),
        ("%album%", TagField::Album),
        ("%genre%", TagField::Genre),
        ("%year%", TagField::Year),
        ("%track%", TagField::TrackNumber),
    ];
    let words = [
        "Kino",
        "Кино",
        "Группа крови",
        "日本",
        "Mr. X",
        "AC_DC",
        "Ü-ber",
        "a b c",
    ];
    let mut rng = Rng(0x5EED_1234_ABCD_0003);
    let mut failures = Vec::new();
    for _ in 0..5_000 {
        // A pattern of 2-4 distinct fields with literal separators between them.
        let mut order: Vec<usize> = (0..fields.len()).collect();
        for i in (1..order.len()).rev() {
            order.swap(i, rng.below(i + 1));
        }
        let count = 2 + rng.below(3);
        let mut pattern = String::new();
        let mut used_separators = Vec::new();
        for (n, &index) in order[..count].iter().enumerate() {
            if n > 0 {
                let separator = *rng.pick(&separators);
                used_separators.push(separator);
                pattern.push_str(&separator.replace('[', "'['").replace(']', "']'"));
            }
            pattern.push_str(fields[index].0);
        }
        let mut tags = BTreeMap::new();
        for &index in &order[..count] {
            let (_, field) = &fields[index];
            let value = match field {
                TagField::Year => format!("{}", 1950 + rng.below(70)),
                TagField::TrackNumber => format!("{:02}", 1 + rng.below(30)),
                _ => rng.pick(&words).to_string(),
            };
            if used_separators
                .iter()
                .any(|separator| value.contains(separator.trim()))
            {
                continue;
            }
            tags.insert(field.clone(), value);
        }
        if tags.len() != count {
            continue;
        }
        let mask = Mask::parse(&pattern).unwrap_or_else(|e| panic!("{pattern:?}: {e}"));
        let rendered = mask.render(&tags).unwrap();
        match mask.extract(&rendered) {
            Ok(back) if back == tags => {}
            other => failures.push(format!(
                "pattern {pattern:?} tags {tags:?} rendered {rendered:?} extracted {other:?}"
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "{} round trips failed, first: {:#?}",
        failures.len(),
        &failures[..failures.len().min(5)]
    );
}

/// A conditional section leaves nothing behind when its placeholder is empty,
/// and renders whole when it is set.
#[test]
fn a_conditional_section_leaves_no_stray_separator() {
    let mask = Mask::parse("[%year% - ]%artist%[ (%album%)]").unwrap();
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Kino".to_string());
    assert_eq!(mask.render(&tags).unwrap(), "Kino");
    tags.insert(TagField::Year, "1988".to_string());
    tags.insert(TagField::Album, "Gruppa krovi".to_string());
    assert_eq!(mask.render(&tags).unwrap(), "1988 - Kino (Gruppa krovi)");
}
