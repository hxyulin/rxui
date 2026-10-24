use crate::*;
use std::ops::Range;

/// A single range, as the matchers return it.
fn one(range: Range<usize>) -> Vec<Range<usize>> {
    vec![range; 1]
}

/// Candidates ranked by `matcher` for `query`, best first, ties in input order.
fn rank<'a>(matcher: &impl Matcher, query: &str, candidates: &[&'a str]) -> Vec<&'a str> {
    let mut scored: Vec<_> = candidates
        .iter()
        .filter_map(|c| Some((matcher.score(query, c)?.score, *c)))
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, c)| c).collect()
}

#[test]
fn fuzzy_ranks_compact_word_start_matches_in_short_paths_first() {
    let paths = [
        "path/to/the_main.rs",
        "themes/mod.rs",
        "src/math.rs",
        "theme.rs",
        "docs/theming.md",
    ];
    assert_eq!(
        rank(&FuzzyMatcher, "thm", &paths),
        [
            "theme.rs",
            "themes/mod.rs",
            "path/to/the_main.rs",
            "docs/theming.md"
        ]
    );
    // "mod" is a whole word in one path and scattered in the other.
    assert_eq!(
        rank(&FuzzyMatcher, "mod", &["my_old_data.rs", "themes/mod.rs"]),
        ["themes/mod.rs", "my_old_data.rs"]
    );
    let m = FuzzyMatcher.score("mod", "themes/mod.rs").unwrap();
    assert_eq!(m.ranges, one(7..10));
    // An extension is not a word start, so "m" matches in the stem, not ".md".
    let m = FuzzyMatcher.score("thm", "theme.md").unwrap();
    assert_eq!(m.ranges, [0..2, 3..4]);
}

#[test]
fn fuzzy_prefers_word_starts_camel_case_and_prefixes() {
    // After a separator: f..b at a word start beats a mid-word b.
    for sep in ["_", "-", "/", " ", "::"] {
        let word = format!("foo{sep}bar");
        let a = FuzzyMatcher.score("fb", &word).unwrap();
        let b = FuzzyMatcher.score("fb", "fooxbar").unwrap();
        assert!(a.score > b.score, "{sep:?}");
        assert_eq!(a.ranges, [0..1, 3 + sep.len()..4 + sep.len()]);
    }
    // A lowercase-to-uppercase step is a word start.
    let camel = FuzzyMatcher.score("fb", "FooBar").unwrap();
    let flat = FuzzyMatcher.score("fb", "Foobar").unwrap();
    assert!(camel.score > flat.score);
    assert_eq!(camel.ranges, [0..1, 3..4]);
    // The best alignment wins over the leftmost one.
    let m = FuzzyMatcher.score("vl", "virtual_list.rs").unwrap();
    assert_eq!(m.ranges, [0..1, 8..9]);
    // A prefix beats the same word later on.
    let prefix = FuzzyMatcher.score("list", "list_view.rs").unwrap();
    let later = FuzzyMatcher.score("list", "virtual_list").unwrap();
    assert!(prefix.score > later.score);
    // Consecutive matches beat scattered ones.
    let run = FuzzyMatcher.score("abc", "xabcx").unwrap();
    let scattered = FuzzyMatcher.score("abc", "xaxbxc").unwrap();
    assert!(run.score > scattered.score);
}

#[test]
fn fuzzy_ignores_case_but_ranks_exact_case_higher() {
    let exact = FuzzyMatcher.score("Main", "Main.rs").unwrap();
    let folded = FuzzyMatcher.score("Main", "main.rs").unwrap();
    assert!(exact.score > folded.score);
    assert_eq!(folded.ranges, one(0..4));
    assert!(FuzzyMatcher.score("MAIN", "main.rs").is_some());
    assert!(FuzzyMatcher.score("ü", "Über.md").is_some());
}

#[test]
fn fuzzy_ranges_are_byte_ranges_on_char_boundaries() {
    let candidate = "日本語/ファイル.rs";
    let m = FuzzyMatcher.score("本フ", candidate).unwrap();
    for range in &m.ranges {
        assert!(candidate.is_char_boundary(range.start));
        assert!(candidate.is_char_boundary(range.end));
    }
    let matched: Vec<&str> = m.ranges.iter().map(|r| &candidate[r.clone()]).collect();
    assert_eq!(matched, ["本", "フ"]);
    // Adjacent multibyte matches merge into one range.
    let m = FuzzyMatcher.score("ファ", candidate).unwrap();
    assert_eq!(&candidate[m.ranges[0].clone()], "ファ");
    assert_eq!(m.ranges.len(), 1);
}

#[test]
fn empty_queries_match_everything_and_misses_return_none() {
    for matcher in [&FuzzyMatcher as &dyn Matcher, &SubstringMatcher] {
        assert_eq!(matcher.score("", "theme.rs"), Some(Match::default()));
        assert_eq!(matcher.score("zz", "theme.rs"), None);
        assert_eq!(matcher.score("theme.rs.bak", "theme.rs"), None);
    }
    // Out of order is no subsequence.
    assert_eq!(FuzzyMatcher.score("mt", "theme"), None);
    assert_eq!(
        PassthroughMatcher.score("zz", "theme.rs"),
        Some(Match::default())
    );
}

#[test]
fn substring_matches_case_insensitively_and_ranks_early_word_starts() {
    let m = SubstringMatcher.score("MOD", "themes/mod.rs").unwrap();
    assert_eq!(m.ranges, one(7..10));
    assert_eq!(
        rank(
            &SubstringMatcher,
            "the",
            &["path/to/the_main.rs", "anthem.rs", "theme.rs"]
        ),
        ["theme.rs", "anthem.rs", "path/to/the_main.rs"]
    );
    let candidate = "Grüße/übersicht.md";
    let m = SubstringMatcher.score("ÜBER", candidate).unwrap();
    assert_eq!(&candidate[m.ranges[0].clone()], "über");
}
