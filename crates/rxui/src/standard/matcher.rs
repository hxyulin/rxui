//! Query matchers for [`super::combobox`]: fuzzy, substring and passthrough.
use std::cell::RefCell;
use std::ops::Range;

/// A candidate's match against a query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Match {
    /// Higher scores rank first. Equal scores keep the items' original order.
    pub score: i32,
    /// Matched UTF-8 byte ranges in the candidate, in order and non-overlapping.
    /// Highlighting ignores a range that does not start and end on a char boundary.
    pub ranges: Vec<Range<usize>>,
}

/// Scores candidates against a query. A [`crate::combobox`] keeps the candidates
/// that return a [`Match`] and ranks them by score.
///
/// The combobox caches results per query, item labels and matcher type, so
/// [`Matcher::score`] must return the same result for the same inputs.
///
/// ```
/// use rxui::{Match, Matcher};
/// /// Keeps labels that start with the query, case-sensitively.
/// struct Prefix;
/// impl Matcher for Prefix {
///     fn score(&self, query: &str, candidate: &str) -> Option<Match> {
///         candidate.starts_with(query).then(|| Match {
///             score: 0,
///             ranges: vec![0..query.len()],
///         })
///     }
/// }
/// assert_eq!(Prefix.score("ma", "main.rs").unwrap().ranges, [0..2]);
/// ```
pub trait Matcher: 'static {
    /// The match of `candidate` against a nonempty `query`, or `None` to hide it.
    fn score(&self, query: &str, candidate: &str) -> Option<Match>;
}

/// Case-insensitive subsequence matching for file names, symbols and commands:
/// every query character must appear in the candidate in order.
///
/// Among the possible alignments it picks the highest score. Each matched
/// character scores more at a word start (the candidate's start, after a
/// character that is not a letter, digit or `.`, such as `_`, `-`, `/` or a
/// space, or at a lowercase-to-uppercase step), when it continues a run of
/// matches (which keeps the word-start bonus of the run's first match), when it
/// matches the first query character at the candidate's start, and when its case
/// matches exactly. Gaps between matches and every character
/// of the candidate cost points, so compact matches in short candidates rank
/// first. Case folding is per character, using the first character of its
/// lowercase form.
///
/// ```
/// use rxui::{FuzzyMatcher, Matcher};
/// let theme = FuzzyMatcher.score("thm", "theme.rs").unwrap();
/// let main = FuzzyMatcher.score("thm", "src/the_main.rs").unwrap();
/// assert!(theme.score > main.score);
/// assert_eq!(theme.ranges, [0..2, 3..4]);
/// assert!(FuzzyMatcher.score("xyz", "theme.rs").is_none());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuzzyMatcher;

/// Case-insensitive substring matching. Earlier matches, matches at a word
/// start and shorter candidates rank first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubstringMatcher;

/// Keeps every item in its original order with nothing highlighted, for
/// applications that filter items themselves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassthroughMatcher;

impl Matcher for PassthroughMatcher {
    fn score(&self, _: &str, _: &str) -> Option<Match> {
        Some(Match::default())
    }
}

/// Points for each matched character.
const MATCH: i32 = 16;
/// Bonus for a match at a word start after a separator, or at the start.
const BOUNDARY: i32 = 8;
/// Bonus for a match at a lowercase-to-uppercase step.
const CAMEL: i32 = 7;
/// Bonus for matching the first query character at the candidate's start.
const PREFIX: i32 = 8;
/// Bonus for a match right after the previous one, which also keeps the word-start
/// bonus of the run's first match.
const CONSECUTIVE: i32 = 4;
/// Bonus for a match whose case is exact.
const CASE: i32 = 1;
/// Cost of a gap between two matches, plus [`GAP_EXTEND`] per extra character.
const GAP_START: i32 = 5;
const GAP_EXTEND: i32 = 1;
/// No alignment ends here.
const NONE: i32 = i32::MIN / 4;

fn fold(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// Word-start bonus of each candidate character.
fn bonus(prev: Option<char>, c: char) -> i32 {
    match prev {
        None => BOUNDARY,
        // A dot starts an extension, not a word, so stems outrank extensions.
        Some(p) if p != '.' && !p.is_alphanumeric() && c.is_alphanumeric() => BOUNDARY,
        Some(p) if p.is_lowercase() && c.is_uppercase() => CAMEL,
        _ => 0,
    }
}

/// Merges matched char positions into byte ranges.
fn ranges(chars: &[(usize, char)], positions: impl Iterator<Item = usize>) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    for j in positions {
        let (start, c) = chars[j];
        let end = start + c.len_utf8();
        match out.last_mut() {
            Some(last) if last.end == start => last.end = end,
            _ => out.push(start..end),
        }
    }
    out
}

/// Reused per-thread buffers, so scoring allocates only the returned ranges.
#[derive(Default)]
struct Scratch {
    query: Vec<(char, char)>,
    chars: Vec<(usize, char)>,
    folded: Vec<char>,
    bonus: Vec<i32>,
    score: Vec<i32>,
    back: Vec<u32>,
    run: Vec<i32>,
}
thread_local! {
    static SCRATCH: RefCell<Scratch> = RefCell::default();
}

impl Matcher for FuzzyMatcher {
    fn score(&self, query: &str, candidate: &str) -> Option<Match> {
        // Most candidates fail the subsequence test; reject them before any setup.
        let mut rest = candidate.chars();
        if !query.chars().all(|q| rest.any(|c| fold(c) == fold(q))) {
            return None;
        }
        if query.is_empty() {
            return Some(Match::default());
        }
        SCRATCH.with_borrow_mut(|s| fuzzy(s, query, candidate))
    }
}

fn fuzzy(s: &mut Scratch, query: &str, candidate: &str) -> Option<Match> {
    s.query.clear();
    s.query.extend(query.chars().map(|c| (c, fold(c))));
    s.chars.clear();
    s.chars.extend(candidate.char_indices());
    s.folded.clear();
    s.folded.extend(s.chars.iter().map(|&(_, c)| fold(c)));
    s.bonus.clear();
    let mut prev = None;
    for &(_, c) in &s.chars {
        s.bonus.push(bonus(prev, c));
        prev = Some(c);
    }
    let (n, m) = (s.query.len(), s.chars.len());
    s.score.clear();
    s.score.resize(n * m, NONE);
    s.back.clear();
    s.back.resize(n * m, u32::MAX);
    s.run.clear();
    s.run.resize(n * m, 0);
    // score[i * m + j]: the best alignment of query[..=i] with query[i] at j.
    for i in 0..n {
        let (exact, folded) = s.query[i];
        // Best predecessor at least two columns back, less its gap cost.
        let (mut carry, mut carry_at) = (NONE, u32::MAX);
        for j in i..m {
            if i > 0 && j >= 2 {
                let k = j - 2;
                let gap = s.score[(i - 1) * m + k] - GAP_START;
                if carry - GAP_EXTEND > gap {
                    carry = (carry - GAP_EXTEND).max(NONE);
                } else {
                    (carry, carry_at) = (gap, k as u32);
                }
            }
            if s.folded[j] != folded {
                continue;
            }
            let case = if s.chars[j].1 == exact { CASE } else { 0 };
            let at = i * m + j;
            s.run[at] = s.bonus[j];
            if i == 0 {
                s.score[j] = MATCH + s.bonus[j] + case + if j == 0 { PREFIX } else { 0 };
                continue;
            }
            // A run continues with its first match's word-start bonus.
            let (diagonal, run) = if j > 0 {
                let run = s.run[at - m - 1].max(s.bonus[j]);
                (s.score[at - m - 1] + MATCH + run + CONSECUTIVE, run)
            } else {
                (NONE, 0)
            };
            let start = carry + MATCH + s.bonus[j];
            if diagonal.max(start) <= NONE / 2 {
                continue;
            }
            if diagonal >= start {
                (s.score[at], s.back[at], s.run[at]) = (diagonal + case, j as u32 - 1, run);
            } else {
                (s.score[at], s.back[at]) = (start + case, carry_at);
            }
        }
    }
    let row = (n - 1) * m;
    let (end, best) = (0..m)
        .map(|j| (j, s.score[row + j]))
        .filter(|&(_, score)| score > NONE / 2)
        .max_by_key(|&(j, score)| (score, std::cmp::Reverse(j)))?;
    let mut positions = vec![0; n];
    let mut j = end;
    for i in (0..n).rev() {
        positions[i] = j;
        j = s.back[i * m + j] as usize;
    }
    Some(Match {
        score: best - m as i32,
        ranges: ranges(&s.chars, positions.into_iter()),
    })
}

impl Matcher for SubstringMatcher {
    fn score(&self, query: &str, candidate: &str) -> Option<Match> {
        if query.is_empty() {
            return Some(Match::default());
        }
        if query.is_ascii() && candidate.is_ascii() {
            // Bytes are chars: search without collecting them.
            let (q, c) = (query.as_bytes(), candidate.as_bytes());
            let start = c.windows(q.len()).position(|w| w.eq_ignore_ascii_case(q))?;
            let prev = start.checked_sub(1).map(|p| char::from(c[p]));
            let prefix = if start == 0 { PREFIX } else { 0 };
            return Some(Match {
                score: MATCH * q.len() as i32 + bonus(prev, char::from(c[start])) + prefix
                    - start as i32
                    - c.len() as i32,
                ranges: std::iter::once(start..start + q.len()).collect(),
            });
        }
        let query: Vec<char> = query.chars().map(fold).collect();
        let chars: Vec<(usize, char)> = candidate.char_indices().collect();
        let start = (0..=chars.len().checked_sub(query.len())?).find(|&j| {
            chars[j..j + query.len()]
                .iter()
                .zip(&query)
                .all(|(&(_, c), &q)| fold(c) == q)
        })?;
        let prev = start.checked_sub(1).map(|p| chars[p].1);
        let prefix = if start == 0 { PREFIX } else { 0 };
        Some(Match {
            score: MATCH * query.len() as i32 + bonus(prev, chars[start].1) + prefix
                - start as i32
                - chars.len() as i32,
            ranges: ranges(&chars, start..start + query.len()),
        })
    }
}
