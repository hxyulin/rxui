//! Fixed- and variable-height virtualization over placement-local scroll metrics.
use crate::{
    Element, IntoElement, Key, ReadContext, ScrollArea, ScrollHandle, ScrollState, SemanticRole,
    VirtualRows, column, scroll_area, virtual_rows::Anchor,
};
use std::ops::Range;

/// A vertical scroll area which describes only visible rows and an overscan margin.
///
/// Row closures may borrow application state and the view context: they run during
/// description conversion, never during painting. The list subscribes its evaluating
/// mount to scroll metrics. Put a large list in a child view to keep scroll updates
/// from rebuilding unrelated application descriptions.
///
/// Every row occupies exactly `row_height` logical pixels, including any spacing.
/// Its content is clipped to that slot. Use a stable data key on the returned row
/// when sorting/inserting items; the default key is the row index. Overlapping keyed
/// rows keep their identities. Rows leaving overscan unmount, releasing local focus,
/// capture, editing state and resources; store durable row state in the model.
///
/// Semantics contain mounted rows with zero-based positions and a total set size.
/// Unmounted rows have no element identity and cannot receive Focus/ScrollIntoView
/// actions. Scroll the viewport (or use [`ScrollHandle::reveal_row`]) to mount them.
/// This is not a full virtual accessibility navigation provider.
#[must_use]
pub struct VirtualList<F> {
    area: ScrollArea,
    state: Option<ScrollState>,
    count: usize,
    row_height: f32,
    overscan: usize,
    render: F,
}

/// Creates a fixed-height virtual list using an application-owned scroll handle.
/// Bind each handle once per UI; separate windows can share it and scroll independently.
/// The first preparation measures the viewport before describing rows.
///
/// `row_height` must be finite and positive; the total extent must not exceed
/// 2^24 logical pixels, where f32 layout loses single-pixel precision. Invalid
/// geometry is rejected as [`crate::UiError::InvalidStyle`] without calling `render`.
/// A bounded parent/height is required, as for [`scroll_area`].
///
/// ```
/// use rxui::prelude::*;
/// struct Files { scroll: ScrollHandle, names: Vec<String>, selected: usize }
/// impl View for Files {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         virtual_list(self.names.len(), 32., &self.scroll, cx, |index| {
///             button(self.names[index].clone()).key(self.names[index].clone())
///                 .on_click(cx.listener(move |this, _, _| this.selected = index))
///         }).overscan(3)
///     }
/// }
/// ```
pub fn virtual_list<F, E>(
    count: usize,
    row_height: f32,
    handle: &ScrollHandle,
    cx: &impl ReadContext,
    render: F,
) -> VirtualList<F>
where
    F: FnMut(usize) -> E,
    E: IntoElement,
{
    VirtualList {
        area: scroll_area(column()).handle(handle.clone()),
        state: handle.state(cx),
        count,
        row_height,
        overscan: 2,
        render,
    }
}

fn extent(count: usize, height: f32) -> Option<f32> {
    let total = count as f64 * f64::from(height);
    (height.is_finite() && height > 0. && total <= 16_777_216.).then_some(total as f32)
}
fn range(
    count: usize,
    height: f32,
    total: f32,
    state: Option<ScrollState>,
    overscan: usize,
) -> Range<usize> {
    let Some(state) = state.filter(|s| s.viewport[1] > 0.) else {
        return 0..0;
    };
    let offset = state.offset[1].clamp(0., (total - state.viewport[1]).max(0.));
    let start = (f64::from(offset) / f64::from(height)).floor() as usize;
    let end =
        ((f64::from(offset) + f64::from(state.viewport[1])) / f64::from(height)).ceil() as usize;
    start.saturating_sub(overscan).min(count)..end.saturating_add(overscan).min(count)
}
/// Builder methods shared by the fixed- and variable-height lists.
macro_rules! list_builders {
    () => {
        /// Number of extra rows on each side of the viewport; defaults to two.
        pub fn overscan(mut self, rows: usize) -> Self {
            self.overscan = rows;
            self
        }
        /// Configures scrollbar gutters without disabling scroll input.
        pub fn scrollbars(mut self, show: bool) -> Self {
            self.area = self.area.scrollbars(show);
            self
        }
        /// Logical vertical scrollbar gutter width.
        pub fn scrollbar_size(mut self, size: f32) -> Self {
            self.area = self.area.scrollbar_size(size);
            self
        }
        /// Configures the list's outer size.
        pub fn size(mut self, width: f32, height: f32) -> Self {
            self.area = self.area.size(width, height);
            self
        }
        /// Configures the list's outer height.
        pub fn height(mut self, height: f32) -> Self {
            self.area = self.area.height(height);
            self
        }
        /// Configures the list's outer width.
        pub fn width(mut self, width: f32) -> Self {
            self.area = self.area.width(width);
            self
        }
        /// Configures the list's flex growth.
        pub fn flex_grow(mut self, factor: f32) -> Self {
            self.area = self.area.flex_grow(factor);
            self
        }
        /// Fills the parent's content height.
        pub fn fill_height(mut self) -> Self {
            self.area = self.area.fill_height();
            self
        }
        /// Fills the parent's content width.
        pub fn fill_width(mut self) -> Self {
            self.area = self.area.fill_width();
            self
        }
    };
}
impl<F> VirtualList<F> {
    list_builders!();
}
impl<F, E> IntoElement for VirtualList<F>
where
    F: FnMut(usize) -> E,
    E: IntoElement,
{
    fn into_element(mut self) -> Element {
        let total = extent(self.count, self.row_height);
        let mut content = list_content(total, self.count);
        if let Some(total) = total {
            for index in range(
                self.count,
                self.row_height,
                total,
                self.state,
                self.overscan,
            ) {
                let mut row = (self.render)(index).into_element();
                let key = row.key.take().unwrap_or_else(|| index.into());
                let top = (index as f64 * f64::from(self.row_height)) as f32;
                content
                    .children
                    .push(slot(row, key, index, top, self.row_height));
            }
        }
        self.area.with_content(content).into_element()
    }
}
/// The list container: the full extent, or NaN to reject invalid geometry.
fn list_content(total: Option<f32>, count: usize) -> Element {
    let mut content = column()
        .fill_width()
        .height(total.unwrap_or(f32::NAN))
        .accessibility_role(SemanticRole::List);
    content
        .semantics
        .get_or_insert_with(Default::default)
        .set_size = Some(count);
    content
}
/// A clipped, absolutely placed row slot.
fn slot(row: Element, key: Key, index: usize, top: f32, height: f32) -> Element {
    let mut slot = column()
        .key(key)
        .absolute()
        .left(0.)
        .right(0.)
        .top(top)
        .height(height)
        .clip()
        .accessibility_role(SemanticRole::ListItem)
        .child(row.fill_width().fill_height().min_width(0.).min_height(0.));
    slot.semantics
        .get_or_insert_with(Default::default)
        .position_in_set = Some(index);
    slot
}

/// A vertical scroll area over rows of differing heights, describing only the
/// visible rows and an overscan margin. Create it with [`variable_list`].
///
/// Heights and stable keys come from a [`VirtualRows`] in application state. Each
/// row occupies exactly its height, including any spacing, and is clipped to that
/// slot. The slot is keyed by the row's [`VirtualRows`] key, so rows keep their
/// identity, focus and local state while they stay mounted, wherever they move.
///
/// When the rows change, the list keeps the first visible row in place: rows
/// inserted, removed or resized above it move the scroll offset by the same
/// distance, so the content under the viewport does not jump. If that row was
/// removed, the next recorded visible row that survives is kept in place. Anchoring
/// is skipped while the viewport is at the top, so rows inserted at the start stay
/// visible, and when a scroll command for the viewport is queued in the same
/// update, which wins instead. Disable it with [`Self::anchoring`].
///
/// Mounting, semantics and the 2^24-pixel extent limit match [`VirtualList`].
#[must_use]
pub struct VariableList<'a, F> {
    area: ScrollArea,
    handle: ScrollHandle,
    state: Option<ScrollState>,
    tree: Option<u64>,
    rows: &'a VirtualRows,
    overscan: usize,
    anchoring: bool,
    render: F,
}

/// Creates a variable-height virtual list: row `index` is `rows.height(index)`
/// logical pixels tall and keyed by `rows.key(index)`. `render` describes row
/// `index` and runs only for mounted rows.
///
/// Heights must be finite and positive and the total must not exceed 2^24 logical
/// pixels; otherwise preparation fails with [`crate::UiError::InvalidStyle`] without
/// calling `render`. A bounded parent/height is required, as for [`scroll_area`].
/// Finding the visible rows takes O(log n) for n rows.
///
/// ```
/// use rxui::prelude::*;
/// struct Results { scroll: ScrollHandle, rows: VirtualRows, lines: Vec<String> }
/// impl View for Results {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         variable_list(&self.rows, &self.scroll, cx, |index| label(self.lines[index].clone()))
///             .fill_height()
///     }
/// }
/// ```
pub fn variable_list<'a, F, E>(
    rows: &'a VirtualRows,
    handle: &ScrollHandle,
    cx: &impl ReadContext,
    render: F,
) -> VariableList<'a, F>
where
    F: FnMut(usize) -> E,
    E: IntoElement,
{
    VariableList {
        area: scroll_area(column()).handle(handle.clone()),
        handle: handle.clone(),
        state: handle.state(cx),
        tree: handle.source_tree(cx),
        rows,
        overscan: 2,
        anchoring: true,
        render,
    }
}
impl<F> VariableList<'_, F> {
    list_builders!();
    /// Whether changes above the first visible row keep it in place; on by default.
    pub fn anchoring(mut self, enabled: bool) -> Self {
        self.anchoring = enabled;
        self
    }
    /// The vertical offset to describe, after any anchoring correction, which is
    /// queued as a scroll command for the placement.
    fn settle_offset(&self, state: &ScrollState, max: f32) -> f32 {
        let rows = self.rows;
        let current = state.offset[1].clamp(0., max);
        let Some(tree) = self.tree else {
            return current;
        };
        if !self.anchoring {
            // Re-enabling records afresh instead of replaying older changes.
            self.handle.with_anchor(tree, |slot, _| *slot = None);
            return current;
        }
        let offset = self
            .handle
            .with_anchor(tree, |slot, pending| {
                let mut offset = current;
                if !pending
                    && let Some(shift) = slot.as_ref().and_then(|a| a.shift(rows))
                    && shift != 0.
                {
                    offset = (f64::from(state.offset[1]) + shift).clamp(0., f64::from(max)) as f32;
                }
                if offset > 0.
                    && let Some(first) = rows.index_at(offset)
                {
                    let bottom = rows.index_at(offset + state.viewport[1]).unwrap_or(first);
                    Anchor::record(slot, rows, first, bottom + 1);
                } else {
                    *slot = None;
                }
                offset
            })
            .unwrap_or(current);
        if offset != current {
            self.handle.anchor_to(tree, [state.offset[0], offset]);
        }
        offset
    }
}
impl<F, E> IntoElement for VariableList<'_, F>
where
    F: FnMut(usize) -> E,
    E: IntoElement,
{
    fn into_element(mut self) -> Element {
        let rows = self.rows;
        let total = rows.is_valid().then(|| rows.total());
        let mut content = list_content(total, rows.len());
        if let (Some(total), Some(state)) = (total, self.state.filter(|s| s.viewport[1] > 0.))
            && !rows.is_empty()
        {
            let offset = self.settle_offset(&state, (total - state.viewport[1]).max(0.));
            let first = rows.index_at(offset).unwrap_or(0);
            let last = rows.index_at(offset + state.viewport[1]).unwrap_or(first);
            let start = first.saturating_sub(self.overscan);
            let end = last.saturating_add(self.overscan).saturating_add(1);
            let mut top = f64::from(rows.offset(start));
            for index in start..end.min(rows.len()) {
                let height = rows.height(index).unwrap_or_default();
                let row = (self.render)(index).into_element();
                let key = rows.key(index).cloned().unwrap_or_else(|| index.into());
                content
                    .children
                    .push(slot(row, key, index, top as f32, height));
                top += f64::from(height);
            }
        }
        self.area.with_content(content).into_element()
    }
}
