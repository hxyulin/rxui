//! Keys and heights of a variable-height virtual list, indexed by a Fenwick tree so
//! offset and index lookups, and single-row height changes, take O(log n).
use crate::Key;
use std::{
    cell::OnceCell,
    collections::HashMap,
    fmt,
    ops::{Bound, RangeBounds},
    sync::atomic::{AtomicU64, Ordering},
};

/// Largest supported content extent: f32 layout keeps single-pixel precision below it.
pub(crate) const MAX_EXTENT: f64 = 16_777_216.;
/// Rows recorded per scroll anchor. The first surviving one determines the shift.
const ANCHOR_ROWS: usize = 8;

/// Revisions are unique across every `VirtualRows`, so replacing the model with a
/// new value is a change as well.
fn next_revision() -> u64 {
    static REVISIONS: AtomicU64 = AtomicU64::new(1);
    REVISIONS.fetch_add(1, Ordering::Relaxed)
}
fn valid(height: f32) -> bool {
    height.is_finite() && height > 0.
}
fn lowbit(i: usize) -> usize {
    i.isolate_lowest_one()
}

/// Stable keys and logical heights for the rows of a [`crate::variable_list`].
///
/// Keep it in application state next to the row data and update both together.
/// Each key identifies its row across insertions, removals and reordering: it keys
/// the mounted row element, and the list uses it to keep the first visible row in
/// place when rows above the viewport change. Keys must be unique.
///
/// A Fenwick (binary indexed) tree stores prefix sums of the heights:
///
/// | Operation | Cost |
/// | --- | --- |
/// | [`offset`](Self::offset), [`index_at`](Self::index_at), [`total`](Self::total), [`set_height`](Self::set_height), [`push`](Self::push) | O(log n) |
/// | [`index_of`](Self::index_of) | O(1) after an O(n) rebuild following insertion or removal |
/// | [`insert`](Self::insert), [`remove`](Self::remove), [`splice`](Self::splice), collecting | O(n) |
///
/// Heights must be finite and positive and the total must not exceed 2^24 logical
/// pixels. Other values are stored, count as zero in offsets, and make the list
/// report [`crate::UiError::InvalidStyle`] without describing rows; see
/// [`Self::is_valid`].
///
/// ```
/// use rxui::VirtualRows;
/// let mut rows: VirtualRows = [("src", 28.), ("main.rs", 22.), ("lib.rs", 22.)]
///     .into_iter()
///     .collect();
/// assert_eq!(rows.offset(2), 50.);
/// assert_eq!(rows.index_at(49.), Some(1));
/// rows.set_height(0, 40.);
/// assert_eq!(rows.total(), 84.);
/// rows.insert(1, "Cargo.toml", 22.);
/// assert_eq!(rows.index_of(&"main.rs".into()), Some(2));
/// ```
#[derive(Clone)]
pub struct VirtualRows {
    keys: Vec<Key>,
    heights: Vec<f32>,
    /// One-based Fenwick tree over valid heights; `tree[0]` is unused.
    tree: Vec<f64>,
    invalid: usize,
    revision: u64,
    /// Key positions, rebuilt on demand after structural changes.
    positions: OnceCell<HashMap<Key, usize>>,
}
impl Default for VirtualRows {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Debug for VirtualRows {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VirtualRows")
            .field("len", &self.len())
            .field("total", &self.total())
            .field("valid", &self.is_valid())
            .finish_non_exhaustive()
    }
}
impl<K: Into<Key>> FromIterator<(K, f32)> for VirtualRows {
    /// Collects rows in O(n).
    fn from_iter<I: IntoIterator<Item = (K, f32)>>(rows: I) -> Self {
        let mut result = Self::new();
        (result.keys, result.heights) = rows.into_iter().map(|(k, h)| (k.into(), h)).unzip();
        result.rebuild();
        result
    }
}
impl<K: Into<Key>> Extend<(K, f32)> for VirtualRows {
    /// Appends rows in O(k log n).
    fn extend<I: IntoIterator<Item = (K, f32)>>(&mut self, rows: I) {
        for (key, height) in rows {
            self.push(key, height);
        }
    }
}
impl VirtualRows {
    /// No rows.
    pub fn new() -> Self {
        Self {
            keys: Vec::new(),
            heights: Vec::new(),
            tree: vec![0.],
            invalid: 0,
            revision: next_revision(),
            positions: OnceCell::new(),
        }
    }
    /// Number of rows.
    pub fn len(&self) -> usize {
        self.keys.len()
    }
    /// Whether there are no rows.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
    /// Key of row `index`.
    pub fn key(&self, index: usize) -> Option<&Key> {
        self.keys.get(index)
    }
    /// Height of row `index`, as supplied.
    pub fn height(&self, index: usize) -> Option<f32> {
        self.heights.get(index).copied()
    }
    /// Whether the list can display these rows: every height is finite and positive
    /// and the total is at most 2^24 logical pixels.
    pub fn is_valid(&self) -> bool {
        self.invalid == 0 && self.prefix(self.len()) <= MAX_EXTENT
    }
    /// Sum of all valid heights, in logical pixels. O(log n).
    pub fn total(&self) -> f32 {
        self.prefix(self.len()) as f32
    }
    /// Top of row `index`: the sum of the valid heights before it. `index` may equal
    /// [`Self::len`], giving [`Self::total`]. O(log n).
    ///
    /// # Panics
    ///
    /// If `index` is greater than [`Self::len`].
    pub fn offset(&self, index: usize) -> f32 {
        assert!(index <= self.len(), "row index out of range");
        self.prefix(index) as f32
    }
    /// The row containing content offset `offset`: the last row whose top is at or
    /// above it. Offsets before the first row give 0 and offsets past the end give
    /// the last row; `None` when there are no rows. O(log n).
    pub fn index_at(&self, offset: f32) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        Some(self.find(f64::from(offset)).min(self.len() - 1))
    }
    /// Index of the row with `key`. The first lookup after an insertion or removal
    /// rebuilds a key map in O(n); later lookups and appends keep it in O(1).
    pub fn index_of(&self, key: &Key) -> Option<usize> {
        self.positions
            .get_or_init(|| {
                self.keys
                    .iter()
                    .enumerate()
                    .map(|(index, key)| (key.clone(), index))
                    .collect()
            })
            .get(key)
            .copied()
    }
    /// Appends a row. O(log n).
    pub fn push(&mut self, key: impl Into<Key>, height: f32) {
        let key = key.into();
        let index = self.len();
        if let Some(positions) = self.positions.get_mut() {
            positions.insert(key.clone(), index);
        }
        self.keys.push(key);
        self.heights.push(height);
        self.invalid += usize::from(!valid(height));
        // The new node covers rows (i - lowbit(i), i]; all but the last are summed.
        let i = index + 1;
        let covered = self.prefix(index) - self.prefix(i - lowbit(i));
        self.tree
            .push(covered + if valid(height) { f64::from(height) } else { 0. });
        self.revision = next_revision();
    }
    /// Inserts a row before `index`. O(n).
    ///
    /// # Panics
    ///
    /// If `index` is greater than [`Self::len`].
    pub fn insert(&mut self, index: usize, key: impl Into<Key>, height: f32) {
        self.splice(index..index, [(key, height)]);
    }
    /// Removes row `index`, returning its key and height. O(n).
    ///
    /// # Panics
    ///
    /// If `index` is out of range.
    pub fn remove(&mut self, index: usize) -> (Key, f32) {
        assert!(index < self.len(), "row index out of range");
        let key = self.keys.remove(index);
        let height = self.heights.remove(index);
        self.rebuild();
        (key, height)
    }
    /// Replaces the rows in `range` with `rows`, as `Vec::splice` does: an empty
    /// range inserts and an empty iterator removes. O(n + k).
    ///
    /// # Panics
    ///
    /// If the range is out of bounds or decreasing.
    pub fn splice<K: Into<Key>>(
        &mut self,
        range: impl RangeBounds<usize>,
        rows: impl IntoIterator<Item = (K, f32)>,
    ) {
        let start = match range.start_bound() {
            Bound::Included(&s) => s,
            Bound::Excluded(&s) => s + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&e) => e + 1,
            Bound::Excluded(&e) => e,
            Bound::Unbounded => self.len(),
        };
        assert!(start <= end && end <= self.len(), "row range out of bounds");
        let (keys, heights): (Vec<Key>, Vec<f32>) =
            rows.into_iter().map(|(k, h)| (k.into(), h)).unzip();
        self.keys.splice(start..end, keys);
        self.heights.splice(start..end, heights);
        self.rebuild();
    }
    /// Changes the height of row `index`. O(log n).
    ///
    /// # Panics
    ///
    /// If `index` is out of range.
    pub fn set_height(&mut self, index: usize, height: f32) {
        let old = self.heights[index];
        if old == height || (old.is_nan() && height.is_nan()) {
            return;
        }
        self.heights[index] = height;
        self.invalid = self.invalid - usize::from(!valid(old)) + usize::from(!valid(height));
        let delta = if valid(height) { f64::from(height) } else { 0. }
            - if valid(old) { f64::from(old) } else { 0. };
        let mut i = index + 1;
        while i < self.tree.len() {
            self.tree[i] += delta;
            i += lowbit(i);
        }
        self.revision = next_revision();
    }
    /// Removes every row.
    pub fn clear(&mut self) {
        self.splice(.., std::iter::empty::<(Key, f32)>());
    }
    /// Changes whenever rows or heights change, unique across all values.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
    /// Sum of the valid heights of rows `0..count`.
    fn prefix(&self, count: usize) -> f64 {
        let mut sum = 0.;
        let mut i = count;
        while i > 0 {
            sum += self.tree[i];
            i -= lowbit(i);
        }
        sum
    }
    /// Number of leading rows whose cumulative height is at most `offset`.
    fn find(&self, offset: f64) -> usize {
        let n = self.len();
        let mut position = 0;
        let mut remaining = offset;
        let mut step = if n == 0 { 0 } else { 1 << n.ilog2() };
        while step > 0 {
            let next = position + step;
            if next <= n && self.tree[next] <= remaining {
                position = next;
                remaining -= self.tree[next];
            }
            step >>= 1;
        }
        position
    }
    /// Rebuilds the tree in O(n) after a structural change.
    fn rebuild(&mut self) {
        let n = self.len();
        self.tree.clear();
        self.tree.push(0.);
        self.invalid = 0;
        for &height in &self.heights {
            self.invalid += usize::from(!valid(height));
            self.tree
                .push(if valid(height) { f64::from(height) } else { 0. });
        }
        for i in 1..=n {
            let parent = i + lowbit(i);
            if parent <= n {
                self.tree[parent] += self.tree[i];
            }
        }
        self.positions = OnceCell::new();
        self.revision = next_revision();
    }
}

/// The first visible rows of one placement, recorded when a list is described, so
/// the next description can keep them in place after rows above them change.
pub(crate) struct Anchor {
    revision: u64,
    /// Row count when recorded.
    len: usize,
    /// Index, key and top of each recorded row.
    rows: Vec<(usize, Key, f64)>,
}
impl Anchor {
    /// Records the rows starting at `first`, which contains the viewport top.
    pub(crate) fn record(slot: &mut Option<Self>, rows: &VirtualRows, first: usize, end: usize) {
        let mut anchor = slot.take().unwrap_or(Self {
            revision: 0,
            len: 0,
            rows: Vec::with_capacity(ANCHOR_ROWS),
        });
        anchor.revision = rows.revision();
        anchor.len = rows.len();
        anchor.rows.clear();
        let mut top = rows.prefix(first);
        for index in first..end.min(first + ANCHOR_ROWS) {
            anchor.rows.push((index, rows.keys[index].clone(), top));
            top += f64::from(rows.heights[index]);
        }
        *slot = Some(anchor);
    }
    /// How far the first surviving recorded row moved, when `rows` changed since
    /// it was recorded.
    pub(crate) fn shift(&self, rows: &VirtualRows) -> Option<f64> {
        if self.revision == rows.revision() {
            return None;
        }
        // Changes only below, or only insertions or removals above, leave a row at
        // its index or moved by the change in length; check both in O(1) before
        // the key map.
        let moved = rows.len() as isize - self.len as isize;
        self.rows.iter().find_map(|(index, key, top)| {
            let index = [*index, index.wrapping_add_signed(moved)]
                .into_iter()
                .find(|&i| rows.key(i) == Some(key))
                .or_else(|| rows.index_of(key))?;
            Some(rows.prefix(index) - top)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn naive(heights: &[f32], count: usize) -> f64 {
        heights[..count]
            .iter()
            .map(|&h| if valid(h) { f64::from(h) } else { 0. })
            .sum()
    }
    fn check(rows: &VirtualRows) {
        for i in 0..=rows.len() {
            assert_eq!(rows.prefix(i), naive(&rows.heights, i), "prefix {i}");
        }
        for i in 0..rows.len() {
            let top = rows.offset(i);
            assert_eq!(rows.index_at(top), Some(i));
            assert_eq!(rows.index_at(top + rows.heights[i] * 0.5), Some(i));
            assert_eq!(rows.index_of(&rows.keys[i]), Some(i));
        }
    }
    /// Deterministic pseudo-random heights in `8..72`.
    fn height(i: usize) -> f32 {
        8. + ((i * 7919 + 13) % 64) as f32
    }
    #[test]
    fn prefix_sums_and_lookups_match_a_linear_scan() {
        for n in [0, 1, 2, 3, 7, 8, 9, 100, 1025] {
            let rows: VirtualRows = (0..n).map(|i| (i, height(i))).collect();
            assert_eq!(rows.len(), n);
            check(&rows);
            assert_eq!(rows.index_at(-5.), (n > 0).then_some(0));
            assert_eq!(rows.index_at(1e9), n.checked_sub(1));
        }
    }
    #[test]
    fn edits_keep_the_index_consistent() {
        let mut rows: VirtualRows = (0..200).map(|i| (i, height(i))).collect();
        rows.set_height(17, 3.5);
        rows.set_height(199, 100.);
        check(&rows);
        rows.insert(0, "head", 10.);
        rows.insert(50, "middle", 11.);
        rows.push("tail", 12.);
        check(&rows);
        assert_eq!(rows.remove(50), (Key::from("middle"), 11.));
        rows.splice(10..20, [("a", 1.), ("b", 2.)]);
        check(&rows);
        assert_eq!(rows.len(), 194);
        rows.splice(.., std::iter::empty::<(Key, f32)>());
        assert!(rows.is_empty());
        assert_eq!(rows.total(), 0.);
        assert_eq!(rows.index_at(0.), None);
    }
    #[test]
    fn appends_keep_a_built_key_map() {
        let mut rows: VirtualRows = (0..5).map(|i| (i, 10.)).collect();
        assert_eq!(rows.index_of(&3.into()), Some(3));
        rows.push(99, 10.);
        assert!(rows.positions.get().is_some());
        assert_eq!(rows.index_of(&99.into()), Some(5));
        rows.insert(0, 100, 10.);
        assert!(rows.positions.get().is_none());
        assert_eq!(rows.index_of(&99.into()), Some(6));
    }
    #[test]
    fn invalid_heights_count_as_zero_and_invalidate() {
        let mut rows: VirtualRows = [(0, 10.), (1, f32::NAN), (2, 10.)].into_iter().collect();
        assert!(!rows.is_valid());
        assert_eq!(rows.total(), 20.);
        rows.set_height(1, 5.);
        assert!(rows.is_valid());
        rows.set_height(2, -1.);
        assert!(!rows.is_valid());
        rows.remove(2);
        assert!(rows.is_valid());
        rows.push(3, 16_777_216.);
        assert!(!rows.is_valid());
    }
    #[test]
    fn revisions_change_with_every_edit_and_differ_between_values() {
        let mut rows: VirtualRows = [(0, 10.)].into_iter().collect();
        let other: VirtualRows = [(0, 10.)].into_iter().collect();
        assert_ne!(rows.revision(), other.revision());
        let before = rows.revision();
        rows.set_height(0, 10.);
        assert_eq!(rows.revision(), before);
        rows.set_height(0, 12.);
        assert_ne!(rows.revision(), before);
    }
    #[test]
    fn anchors_follow_their_key_and_fall_back_to_later_rows() {
        let mut rows: VirtualRows = (0..100).map(|i| (i, 20.)).collect();
        let mut slot = None;
        Anchor::record(&mut slot, &rows, 10, 15);
        let anchor = slot.as_ref().unwrap();
        assert_eq!(anchor.shift(&rows), None);
        rows.splice(0..0, [(1000, 30.), (1001, 30.)]);
        assert_eq!(anchor.shift(&rows), Some(60.));
        rows.remove(12); // row key 10 is gone; key 11 moved up by 60 - 20
        assert_eq!(anchor.shift(&rows), Some(40.));
        rows.set_height(0, 130.);
        assert_eq!(anchor.shift(&rows), Some(140.));
    }
}

/// Window-dependent measurements, owned by a scroll placement, never VirtualRows.
pub(crate) struct Measurements {
    pub rows: VirtualRows,
    estimates: VirtualRows,
    source: u64,
    version: u64,
    environment: Option<(f32, u64, f32, crate::typography::TextStyle)>,
    pub reader: crate::MountId,
}
pub(crate) type MeasurementCache = std::rc::Rc<std::cell::RefCell<Measurements>>;
#[derive(Clone)]
pub(crate) struct MeasuredRow {
    pub cache: MeasurementCache,
    pub index: usize,
}
impl Measurements {
    pub fn new(rows: &VirtualRows, reader: crate::MountId) -> Self {
        Self {
            rows: rows.clone(),
            estimates: rows.clone(),
            source: rows.revision(),
            version: 0,
            environment: None,
            reader,
        }
    }
    pub fn sync(&mut self, rows: &VirtualRows, version: u64) {
        if self.source == rows.revision() && self.version == version {
            return;
        }
        let mut next = rows.clone();
        if self.version == version {
            for index in 0..rows.len() {
                let key = rows.key(index).unwrap();
                if let Some(old) = self.estimates.index_of(key)
                    && self.estimates.height(old) == rows.height(index)
                {
                    next.set_height(index, self.rows.height(old).unwrap());
                }
            }
        }
        self.estimates = rows.clone();
        self.rows = next;
        self.source = rows.revision();
        self.version = version;
    }
    pub fn environment(
        &mut self,
        width: f32,
        generation: u64,
        font: f32,
        style: &crate::typography::TextStyle,
    ) -> bool {
        let next = (width, generation, font, style.clone());
        if self.environment.as_ref() == Some(&next) {
            return false;
        }
        let changed = self.environment.is_some();
        self.environment = Some(next);
        if changed {
            self.rows = self.estimates.clone();
        }
        changed
    }
}

#[cfg(test)]
mod measurement_tests {
    use super::*;
    #[test]
    fn structural_changes_preserve_only_unchanged_estimates_and_version_resets() {
        let mut runtime = crate::Runtime::new();
        let model = runtime.update(|cx| cx.new(|_| ()));
        let mount = runtime.update(|cx| cx.mount(&model).unwrap());
        let mut rows: VirtualRows = [(0, 20.), (1, 20.), (2, 20.)].into_iter().collect();
        let mut cache = Measurements::new(&rows, mount.id());
        cache.rows.set_height(1, 80.);
        rows.insert(0, 3, 20.);
        cache.sync(&rows, 0);
        assert_eq!(cache.rows.height(2), Some(80.));
        rows.remove(0);
        cache.sync(&rows, 0);
        assert_eq!(cache.rows.height(1), Some(80.));
        rows.set_height(1, 30.);
        cache.sync(&rows, 0);
        assert_eq!(cache.rows.height(1), Some(30.));
        cache.rows.set_height(1, 100.);
        cache.sync(&rows, 1);
        assert_eq!(cache.rows.height(1), Some(30.));
    }
}
