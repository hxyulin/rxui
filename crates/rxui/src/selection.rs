//! Keyed selection shared by [`crate::virtual_tree`] and [`crate::list_view`]:
//! selected keys, the active key and a range anchor, moved by keyboard, pointer
//! and type-ahead commands over rows in visible order.
use crate::{Key, Modifiers};
use std::{collections::HashSet, time::Duration};

/// Selection policy. Logical activity is independent of selection in both modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    /// At most one selected key.
    Single,
    /// Primary-click toggles; Shift selects the inclusive visible range.
    Multiple,
}

/// Rows a selection moves over, in visible order.
pub(crate) trait SelectionRows {
    fn len(&self) -> usize;
    fn key(&self, index: usize) -> &Key;
    fn index(&self, key: &Key) -> Option<usize>;
    fn label(&self, index: usize) -> &str;
}

/// Selected keys, the active key and the range anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub(crate) mode: SelectionMode,
    pub(crate) selected: HashSet<Key>,
    pub(crate) active: Option<Key>,
    pub(crate) anchor: Option<Key>,
}
impl Selection {
    pub(crate) fn new(mode: SelectionMode) -> Self {
        Self {
            mode,
            selected: HashSet::new(),
            active: None,
            anchor: None,
        }
    }
    /// Moves activity from row `from` to row `to` and updates the selection.
    ///
    /// In multiple mode, Shift (unless `extend` is false, as for type-ahead)
    /// selects the range from the anchor, adding to the selection with Primary.
    /// Otherwise Primary moves activity only, and `toggle` (a click or Space)
    /// toggles the destination. A plain move selects only the destination.
    pub(crate) fn move_to(
        &mut self,
        rows: &impl SelectionRows,
        from: usize,
        to: usize,
        modifiers: Modifiers,
        toggle: bool,
        extend: bool,
    ) {
        let key = rows.key(to).clone();
        let previous = self.active.replace(key.clone());
        if self.mode == SelectionMode::Multiple && modifiers.shift && extend {
            // A hidden anchor is retained, but cannot define a visible range.
            // Fall back to the previous active row without overwriting the anchor.
            if self.anchor.is_none() {
                self.anchor = previous;
            }
            let anchor = self
                .anchor
                .as_ref()
                .and_then(|a| rows.index(a))
                .unwrap_or(from);
            if !modifiers.primary() {
                self.selected.clear();
            }
            for i in anchor.min(to)..=anchor.max(to) {
                self.selected.insert(rows.key(i).clone());
            }
        } else if modifiers.primary() {
            // Primary navigation moves activity only; primary Select/Space toggles.
            if toggle {
                if self.mode == SelectionMode::Single {
                    self.selected.clear();
                }
                if !self.selected.remove(&key) {
                    self.selected.insert(key.clone());
                }
                self.anchor = Some(key);
            }
        } else {
            self.selected.clear();
            self.selected.insert(key.clone());
            self.anchor = Some(key);
        }
    }
    /// Selects every row in multiple mode, keeping activity and the anchor.
    pub(crate) fn select_all(&mut self, rows: &impl SelectionRows) {
        if self.mode == SelectionMode::Multiple {
            self.selected = (0..rows.len()).map(|i| rows.key(i).clone()).collect();
        }
    }
}

/// Type-ahead prefix search over row labels.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TypeAhead {
    text: String,
    at: Duration,
}
impl TypeAhead {
    pub(crate) fn clear(&mut self) {
        self.text.clear();
    }
    /// Finds the next row after `from` whose label starts with the typed prefix,
    /// wrapping. Input within 700 ms extends the prefix and keeps the current
    /// match; repeating a single letter cycles through its matches. `time` is
    /// monotonic input time from a consistent origin.
    pub(crate) fn find(
        &mut self,
        rows: &impl SelectionRows,
        from: usize,
        text: &str,
        time: Duration,
    ) -> Option<usize> {
        let len = rows.len();
        if len == 0 {
            return None;
        }
        if time.saturating_sub(self.at) > Duration::from_millis(700) || time < self.at {
            self.text.clear();
        }
        let text = text.to_lowercase();
        let repeated = self.text == text && text.chars().count() == 1;
        let extending = !self.text.is_empty() && !repeated;
        if !repeated {
            self.text.push_str(&text);
        }
        self.at = time;
        let matches = |i: &usize| rows.label(*i).to_lowercase().starts_with(&self.text);
        let start = usize::from(!extending);
        let found = (start..len + start)
            .map(|step| (from + step) % len)
            .find(matches);
        if found.is_some() || !extending {
            return found;
        }
        // The longer prefix has no match: search again with the latest input.
        self.text = text;
        let matches = |i: &usize| rows.label(*i).to_lowercase().starts_with(&self.text);
        (1..=len).map(|step| (from + step) % len).find(matches)
    }
}
