//! Keyed data-presentation compositions.
//!
//! Property grids, trees, and tables. Each takes a caller-owned visible range
//! rather than scrolling itself: windowing is the caller's policy until
//! `virtual_list` lands, and the range is narrowed here so a scroll position
//! computed at runtime can never index past the rows.

use std::ops::Range;

mod property_grid;
mod table;
mod tree;

pub use property_grid::*;
pub use table::*;
pub use tree::*;

/// Narrows a caller-supplied visible range onto the rows that exist.
///
/// Callers own scrolling, so the range can name rows past the end or be
/// inverted after a shrink; both must yield an empty selection, never a panic.
fn visible_rows(visible: Range<usize>, len: usize) -> Range<usize> {
    let end = visible.end.min(len);
    visible.start.min(end)..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_visible_range_inside_the_rows_is_used_as_given() {
        assert_eq!(visible_rows(2..5, 8), 2..5);
        assert_eq!(visible_rows(0..8, 8), 0..8);
    }

    #[test]
    fn a_visible_range_past_the_last_row_is_truncated() {
        assert_eq!(visible_rows(6..40, 8), 6..8);
        assert_eq!(visible_rows(40..80, 8), 8..8);
        assert_eq!(visible_rows(0..4, 0), 0..0);
    }

    #[test]
    fn an_inverted_visible_range_selects_nothing() {
        // Written as struct literals: `5..2` is a compile-time lint, but a
        // scroll position computed at runtime can still invert.
        assert_eq!(visible_rows(Range { start: 5, end: 2 }, 8), 2..2);
        assert_eq!(visible_rows(Range { start: 9, end: 1 }, 8), 1..1);
    }
}
