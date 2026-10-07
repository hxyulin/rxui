//! Fixed-height virtualization over placement-local scroll metrics.
use crate::{
    Element, IntoElement, ReadContext, ScrollArea, ScrollHandle, ScrollState, SemanticRole, column,
    scroll_area,
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
impl<F> VirtualList<F> {
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
}
impl<F, E> IntoElement for VirtualList<F>
where
    F: FnMut(usize) -> E,
    E: IntoElement,
{
    fn into_element(mut self) -> Element {
        let total = extent(self.count, self.row_height);
        let mut content = column()
            .fill_width()
            .height(total.unwrap_or(f32::NAN))
            .accessibility_role(SemanticRole::List);
        content
            .semantics
            .get_or_insert_with(Default::default)
            .set_size = Some(self.count);
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
                let mut slot = column()
                    .key(key)
                    .absolute()
                    .left(0.)
                    .right(0.)
                    .top((index as f64 * f64::from(self.row_height)) as f32)
                    .height(self.row_height)
                    .clip()
                    .accessibility_role(SemanticRole::ListItem)
                    .child(row.fill_width().fill_height().min_width(0.).min_height(0.));
                slot.semantics
                    .get_or_insert_with(Default::default)
                    .position_in_set = Some(index);
                content.children.push(slot);
            }
        }
        self.area.with_content(content).into_element()
    }
}
