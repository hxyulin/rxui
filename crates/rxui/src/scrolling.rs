//! Placement-scoped scroll references. Sharing a handle never shares offsets.
use crate::{
    AppContext, Bounds, ElementId, MountId, ReadContext, Runtime, Ui, UiError, View, VirtualRows,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    rc::{Rc, Weak},
};

/// Current logical scroll metrics for one bound viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollState {
    /// Current offsets.
    pub offset: [f32; 2],
    /// Maximum offsets.
    pub range: [f32; 2],
    /// Visible scrollport dimensions: the padding box, which content scrolls under.
    pub viewport: [f32; 2],
    /// Absolute logical scrollport (padding box).
    pub bounds: Bounds,
}
/// Scroll reference resolution/command failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollError {
    /// No live binding exists in the source mount's UI; initialization has no implicit source.
    NoPlacement,
    /// Previously captured placement was removed/replaced.
    Disposed,
    /// The explicit placement belongs to another runtime.
    WrongRuntime,
    /// Nonfinite offset/delta.
    InvalidOffset,
}
impl fmt::Display for ScrollError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoPlacement => "scroll reference has no source placement",
            Self::Disposed => "scroll placement was disposed",
            Self::WrongRuntime => "scroll placement belongs to another runtime",
            Self::InvalidOffset => "scroll offset must be finite",
        })
    }
}
impl Error for ScrollError {}
struct Binding {
    element: ElementId,
    scopes: Rc<Vec<MountId>>,
    life: Weak<()>,
    commands: Weak<RefCell<Vec<Command>>>,
    state: ScrollState,
}
#[derive(Default)]
struct Data {
    bindings: HashMap<u64, Binding>,
    readers: HashSet<MountId>,
    /// Scroll anchor of a variable-height list, per UI tree.
    anchors: HashMap<u64, crate::virtual_rows::Anchor>,
    measurements: HashMap<u64, crate::virtual_rows::MeasurementCache>,
}
/// How a scroll container moves for discrete mouse-wheel steps and programmatic
/// commands ([`ScrollHandle::scroll_to`], [`ScrollHandle::scroll_by`] and the reveal
/// methods). Precise trackpad deltas, scrollbar dragging, keyboard range keys,
/// assistive actions, scroll anchoring and focus or caret reveal always move
/// directly; see `docs/animation.md`.
///
/// Smooth motion needs a host that drives the frame clock (`Runtime::begin_frame`)
/// and is skipped while reduce motion is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollBehavior {
    /// Animate toward the new offset over [`crate::ThemeMotion::scroll`].
    Smooth,
    /// Jump to the new offset.
    Instant,
}
impl ScrollBehavior {
    /// Smooth on macOS and Windows, whose system scroll views and browsers animate
    /// wheel steps; Instant elsewhere, where GTK and Qt scroll views move by whole
    /// steps. Scroll containers use this by default.
    pub const fn platform() -> Self {
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            Self::Smooth
        } else {
            Self::Instant
        }
    }
}
impl Default for ScrollBehavior {
    /// [`ScrollBehavior::platform`].
    fn default() -> Self {
        Self::platform()
    }
}
/// Cloneable viewport reference. Bind once per UI with `.scroll_handle(...)`.
/// The same handle can appear in distinct windows; resolution uses the listener/view
/// mount's placement. Reading state in a ViewContext subscribes that mount to metrics.
/// Capture a ScrollPlacement inside a listener for later async/application work.
#[derive(Clone, Default)]
pub struct ScrollHandle(Rc<RefCell<Data>>);
impl fmt::Debug for ScrollHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScrollHandle").finish_non_exhaustive()
    }
}
impl ScrollHandle {
    /// Creates an unbound reference.
    pub fn new() -> Self {
        Self::default()
    }
    pub(crate) fn id(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    /// Current metrics in the source placement, absent before first layout/after removal.
    /// A read during view evaluation subscribes only that mount, not other windows.
    pub fn state(&self, cx: &impl ReadContext) -> Option<ScrollState> {
        let scope = cx.placement_scope()?;
        cx.track_scroll(self);
        let data = self.0.borrow();
        data.bindings
            .values()
            .find(|b| b.life.strong_count() != 0 && b.scopes.contains(&scope))
            .map(|b| b.state)
    }
    /// Captures an explicit, weak placement for async completions or app commands.
    pub fn placement(&self, cx: &impl ReadContext) -> Result<ScrollPlacement, ScrollError> {
        let scope = cx.placement_scope().ok_or(ScrollError::NoPlacement)?;
        let data = self.0.borrow();
        let (&tree, binding) = data
            .bindings
            .iter()
            .find(|(_, b)| b.life.strong_count() != 0 && b.scopes.contains(&scope))
            .ok_or(ScrollError::NoPlacement)?;
        Ok(ScrollPlacement {
            handle: self.clone(),
            tree,
            element: binding.element,
            runtime: scope.runtime,
        })
    }
    /// Queues absolute logical offsets in the current listener's placement, moving as
    /// the viewport's [`ScrollBehavior`] selects.
    pub fn scroll_to(&self, cx: &mut AppContext<'_>, offset: [f32; 2]) -> Result<(), ScrollError> {
        self.placement(cx)?.scroll_to(cx, offset)
    }
    /// Queues relative motion; clamping occurs when applied to current content geometry.
    /// While a smooth scroll is in flight, the motion adds to its target.
    pub fn scroll_by(&self, cx: &mut AppContext<'_>, delta: [f32; 2]) -> Result<(), ScrollError> {
        self.placement(cx)?.scroll_by(cx, delta)
    }
    /// Queues absolute logical offsets that apply without animation, whatever the
    /// viewport's [`ScrollBehavior`], ending a smooth scroll in flight.
    pub fn jump_to(&self, cx: &mut AppContext<'_>, offset: [f32; 2]) -> Result<(), ScrollError> {
        self.placement(cx)?.jump_to(cx, offset)
    }
    /// Reveals the vertical content span `start..start + extent` in the source
    /// placement, moving the minimum distance: a span above the viewport aligns with
    /// its top, one below with its bottom, and one taller than the viewport with its
    /// top. Negative or nonfinite values return InvalidOffset.
    pub fn reveal_span(
        &self,
        cx: &mut AppContext<'_>,
        start: f32,
        extent: f32,
    ) -> Result<(), ScrollError> {
        self.placement(cx)?.reveal_span(cx, start, extent)
    }
    /// Reveals row `index` of a variable-height list in the source placement, moving
    /// the minimum distance. An index past the end or rows the list would reject
    /// (see [`crate::VirtualRows::is_valid`]) return InvalidOffset. O(log n).
    pub fn reveal_list_row(
        &self,
        cx: &mut AppContext<'_>,
        rows: &crate::VirtualRows,
        index: usize,
    ) -> Result<(), ScrollError> {
        self.placement(cx)?.reveal_list_row(cx, rows, index)
    }
    /// Reveals a row using this placement's measured heights and remaining estimates.
    /// Call after the measured list has been prepared; the index is application-owned.
    pub fn reveal_measured_row(
        &self,
        cx: &mut AppContext<'_>,
        index: usize,
    ) -> Result<(), ScrollError> {
        self.placement(cx)?.reveal_measured_row(cx, index)
    }
    /// Reveals a fixed-height row in the source placement, moving the minimum distance.
    /// The index is application-owned; offsets clamp to the current scroll range.
    /// Invalid height or an extent beyond 2^24 logical pixels returns InvalidOffset.
    pub fn reveal_row(
        &self,
        cx: &mut AppContext<'_>,
        index: usize,
        row_height: f32,
    ) -> Result<(), ScrollError> {
        self.placement(cx)?.reveal_row(cx, index, row_height)
    }
    pub(crate) fn state_in(&self, tree: u64) -> Option<ScrollState> {
        self.0.borrow().bindings.get(&tree).map(|b| b.state)
    }
    /// The UI tree of the source placement, as [`Self::state`] resolves it.
    pub(crate) fn source_tree(&self, cx: &impl ReadContext) -> Option<u64> {
        let scope = cx.placement_scope()?;
        let data = self.0.borrow();
        data.bindings
            .iter()
            .find(|(_, b)| b.life.strong_count() != 0 && b.scopes.contains(&scope))
            .map(|(&tree, _)| tree)
    }
    pub(crate) fn measurements(
        &self,
        tree: u64,
        rows: &VirtualRows,
        reader: MountId,
        version: u64,
    ) -> crate::virtual_rows::MeasurementCache {
        let mut data = self.0.borrow_mut();
        let cache = data
            .measurements
            .entry(tree)
            .or_insert_with(|| {
                Rc::new(RefCell::new(crate::virtual_rows::Measurements::new(
                    rows, reader,
                )))
            })
            .clone();
        cache.borrow_mut().sync(rows, version);
        cache.borrow_mut().reader = reader;
        cache
    }
    /// Runs `f` with the anchor slot of `tree` and whether commands are queued for
    /// its viewport. Returns `None` without running `f` when `tree` is unbound.
    pub(crate) fn with_anchor<R>(
        &self,
        tree: u64,
        f: impl FnOnce(&mut Option<crate::virtual_rows::Anchor>, bool) -> R,
    ) -> Option<R> {
        let mut data = self.0.borrow_mut();
        let binding = data.bindings.get(&tree)?;
        let element = binding.element;
        let pending = binding
            .commands
            .upgrade()
            .is_some_and(|c| c.borrow().iter().any(|c| c.target == element));
        let mut slot = data.anchors.remove(&tree);
        let result = f(&mut slot, pending);
        if let Some(anchor) = slot {
            data.anchors.insert(tree, anchor);
        }
        Some(result)
    }
    /// Queues an anchoring correction to absolute `offset`; a smooth scroll in flight
    /// moves by `shift` along with it.
    pub(crate) fn anchor_to(&self, tree: u64, offset: [f32; 2], shift: [f32; 2]) {
        let data = self.0.borrow();
        if let Some(b) = data.bindings.get(&tree)
            && let Some(commands) = b.commands.upgrade()
        {
            commands.borrow_mut().push(Command {
                target: b.element,
                offset,
                mode: CommandMode::Anchor(shift),
            });
        }
    }
    pub(crate) fn element_in(&self, tree: u64) -> Option<ElementId> {
        self.0.borrow().bindings.get(&tree).map(|b| b.element)
    }
}
pub(crate) struct ScrollReads {
    mount: MountId,
    handles: Vec<ScrollHandle>,
}
impl ScrollReads {
    pub(crate) fn new(mount: MountId, handles: Vec<ScrollHandle>) -> Self {
        for h in &handles {
            h.0.borrow_mut().readers.insert(mount);
        }
        Self { mount, handles }
    }
}
impl Drop for ScrollReads {
    fn drop(&mut self) {
        for h in &self.handles {
            h.0.borrow_mut().readers.remove(&self.mount);
        }
    }
}
/// Weak explicit viewport placement. It cannot resurrect a closed window/removed node.
#[derive(Clone, Debug)]
pub struct ScrollPlacement {
    handle: ScrollHandle,
    tree: u64,
    element: ElementId,
    runtime: u64,
}
impl ScrollPlacement {
    /// Latest metrics, failing after viewport replacement/removal.
    pub fn state(&self) -> Result<ScrollState, ScrollError> {
        let data = self.handle.0.borrow();
        let b = data
            .bindings
            .get(&self.tree)
            .filter(|b| b.element == self.element && b.life.strong_count() != 0)
            .ok_or(ScrollError::Disposed)?;
        Ok(b.state)
    }
    /// Queue an absolute position through an explicit source, including outside listeners.
    /// It follows the viewport's [`ScrollBehavior`].
    pub fn scroll_to(&self, cx: &mut AppContext<'_>, offset: [f32; 2]) -> Result<(), ScrollError> {
        self.command(cx, offset, CommandMode::To)
    }
    /// Queue relative motion through an explicit source. While a smooth scroll is in
    /// flight, the motion adds to its target.
    pub fn scroll_by(&self, cx: &mut AppContext<'_>, delta: [f32; 2]) -> Result<(), ScrollError> {
        self.command(cx, delta, CommandMode::By)
    }
    /// Queue an absolute position that applies without animation.
    pub fn jump_to(&self, cx: &mut AppContext<'_>, offset: [f32; 2]) -> Result<(), ScrollError> {
        self.command(cx, offset, CommandMode::Jump)
    }
    /// Reveals the vertical span `start..start + extent` through this placement,
    /// moving the minimum distance from the current offset.
    pub fn reveal_span(
        &self,
        cx: &mut AppContext<'_>,
        start: f32,
        extent: f32,
    ) -> Result<(), ScrollError> {
        if !(start.is_finite() && extent.is_finite() && start >= 0. && extent >= 0.) {
            return Err(ScrollError::InvalidOffset);
        }
        let state = self.state()?;
        let end = start + extent;
        let y = if start < state.offset[1] || extent > state.viewport[1] {
            start
        } else if end > state.offset[1] + state.viewport[1] {
            end - state.viewport[1]
        } else {
            state.offset[1]
        };
        self.scroll_to(cx, [state.offset[0], y])
    }
    /// Reveals row `index` of a variable-height list through this placement.
    pub fn reveal_list_row(
        &self,
        cx: &mut AppContext<'_>,
        rows: &crate::VirtualRows,
        index: usize,
    ) -> Result<(), ScrollError> {
        let Some(height) = rows.height(index).filter(|_| rows.is_valid()) else {
            return Err(ScrollError::InvalidOffset);
        };
        self.reveal_span(cx, rows.offset(index), height)
    }
    /// Reveals a row using cached placement geometry. Unknown heights use estimates.
    /// Returns InvalidOffset before measurements exist or for an invalid index.
    /// After structural edits, prepare the list before using its new indices.
    pub fn reveal_measured_row(
        &self,
        cx: &mut AppContext<'_>,
        index: usize,
    ) -> Result<(), ScrollError> {
        self.state()?;
        let (top, height) = {
            let data = self.handle.0.borrow();
            let cache = data
                .measurements
                .get(&self.tree)
                .ok_or(ScrollError::InvalidOffset)?
                .borrow();
            let height = cache
                .rows
                .height(index)
                .filter(|_| cache.rows.is_valid())
                .ok_or(ScrollError::InvalidOffset)?;
            (cache.rows.offset(index), height)
        };
        self.reveal_span(cx, top, height)
    }
    /// Reveals a fixed-height row through this explicit weak placement.
    /// The index must describe application data; actual offsets clamp at application.
    pub fn reveal_row(
        &self,
        cx: &mut AppContext<'_>,
        index: usize,
        row_height: f32,
    ) -> Result<(), ScrollError> {
        let end = (index as f64 + 1.) * f64::from(row_height);
        if !row_height.is_finite() || row_height <= 0. || end > 16_777_216. {
            return Err(ScrollError::InvalidOffset);
        }
        let start = (index as f64 * f64::from(row_height)) as f32;
        self.reveal_span(cx, start, end as f32 - start)
    }
    fn command(
        &self,
        cx: &mut AppContext<'_>,
        offset: [f32; 2],
        mode: CommandMode,
    ) -> Result<(), ScrollError> {
        if offset.iter().any(|v| !v.is_finite()) {
            return Err(ScrollError::InvalidOffset);
        }
        if cx.runtime.id != self.runtime {
            return Err(ScrollError::WrongRuntime);
        }
        let data = self.handle.0.borrow();
        let b = data
            .bindings
            .get(&self.tree)
            .filter(|b| b.element == self.element && b.life.strong_count() != 0)
            .ok_or(ScrollError::Disposed)?;
        b.commands
            .upgrade()
            .ok_or(ScrollError::Disposed)?
            .borrow_mut()
            .push(Command {
                target: self.element,
                offset,
                mode,
            });
        Ok(())
    }
}
pub(crate) struct Command {
    pub target: ElementId,
    pub offset: [f32; 2],
    pub mode: CommandMode,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CommandMode {
    /// Absolute, following the viewport's behavior.
    To,
    /// Relative to the current offset, or to a smooth scroll's target.
    By,
    /// Absolute and immediate.
    Jump,
    /// Absolute and immediate: a scroll-anchoring correction. A smooth scroll in
    /// flight moves by this amount with it.
    Anchor([f32; 2]),
}
pub(crate) struct Scrolling {
    life: Rc<()>,
    commands: Rc<RefCell<Vec<Command>>>,
    scopes: Rc<Vec<MountId>>,
    bound: HashMap<usize, ScrollHandle>,
    revision: Option<(u64, u64, u64)>,
    tree: u64,
}
impl Scrolling {
    fn new(tree: u64) -> Self {
        Self {
            life: Rc::new(()),
            commands: Rc::new(RefCell::new(Vec::new())),
            scopes: Rc::new(Vec::new()),
            bound: HashMap::new(),
            revision: None,
            tree,
        }
    }
}
impl Drop for Scrolling {
    fn drop(&mut self) {
        for handle in self.bound.values() {
            let mut data = handle.0.borrow_mut();
            data.bindings.remove(&self.tree);
            data.anchors.remove(&self.tree);
            data.measurements.remove(&self.tree);
            for scope in self.scopes.iter() {
                data.readers.remove(scope);
            }
        }
    }
}
impl<T: View> Ui<T> {
    pub(crate) fn scroll_commands_pending(&self) -> bool {
        self.scrolling
            .as_ref()
            .is_some_and(|s| !s.commands.borrow().is_empty())
    }
    pub(crate) fn publish_scroll(&mut self, runtime: &Runtime) -> Result<bool, UiError> {
        self.publish_scroll_refinement(runtime, &HashSet::new())
    }
    /// Publishes all metrics, returning whether non-measurement work was dirtied.
    /// Only range/offset feedback from a viewport measured on the previous pass
    /// can be attributed to refinement; viewport geometry changes still converge
    /// under the normal control-layout budget.
    pub(crate) fn publish_scroll_refinement(
        &mut self,
        runtime: &Runtime,
        refining_viewports: &HashSet<ElementId>,
    ) -> Result<bool, UiError> {
        let revision = self.scroll_revision();
        if self
            .scrolling
            .as_ref()
            .is_some_and(|s| s.revision == Some(revision))
        {
            return Ok(false);
        }
        self.scroll_handle_nodes.retain(|id| {
            self.nodes
                .get(id)
                .is_some_and(|n| n.element.scroll_handle.is_some())
        });
        if self.scroll_handle_nodes.is_empty() && self.scrolling.is_none() {
            return Ok(false);
        }
        let mut seen = HashSet::new();
        let handles: Vec<_> = self
            .scroll_handle_nodes
            .iter()
            .filter_map(|id| {
                self.nodes
                    .get(id)?
                    .element
                    .scroll_handle
                    .as_ref()
                    .map(|h| (*id, h.clone()))
            })
            .collect();
        for (_, h) in &handles {
            if !seen.insert(h.id()) {
                return Err(UiError::DuplicateScrollHandle);
            }
        }
        if handles.is_empty() && self.scrolling.is_none() {
            return Ok(false);
        }
        let mut scopes: Vec<_> = self.mount_ids().collect();
        scopes.sort_unstable_by_key(|m| m.serial);
        let scrolling = self
            .scrolling
            .get_or_insert_with(|| Box::new(Scrolling::new(self.tree)));
        if *scrolling.scopes != scopes {
            scrolling.scopes = Rc::new(scopes);
        }
        let mut invalidated = false;
        scrolling.bound.retain(|key, h| {
            if !seen.contains(key) {
                let mut data = h.0.borrow_mut();
                data.anchors.remove(&self.tree);
                data.measurements.remove(&self.tree);
                if data.bindings.remove(&self.tree).is_some() {
                    for &reader in &data.readers {
                        if scrolling.scopes.contains(&reader) {
                            runtime.inner.invalidate_mount(reader);
                            invalidated = true;
                        }
                    }
                }
                false
            } else {
                true
            }
        });
        for (id, handle) in handles {
            scrolling.bound.insert(handle.id(), handle.clone());
            let n = &self.nodes[&id];
            if !n.visible {
                let mut data = handle.0.borrow_mut();
                data.measurements.remove(&self.tree);
                data.anchors.remove(&self.tree);
                if data.bindings.remove(&self.tree).is_some() {
                    for &reader in &data.readers {
                        if scrolling.scopes.contains(&reader) {
                            runtime.inner.invalidate_mount(reader);
                            invalidated = true;
                        }
                    }
                }
                continue;
            }
            let scrollport = n.descendant_clip_box();
            let state = ScrollState {
                offset: n.scroll_offset,
                range: n.scroll_range,
                viewport: [scrollport.width, scrollport.height],
                bounds: scrollport,
            };
            let mut data = handle.0.borrow_mut();
            if data
                .bindings
                .get(&self.tree)
                .is_some_and(|b| b.element != id)
            {
                data.measurements.remove(&self.tree);
                data.anchors.remove(&self.tree);
            }
            let changed = data.bindings.get(&self.tree).is_none_or(|b| {
                b.element != id || b.state != state || b.scopes != scrolling.scopes
            });
            let measurement_feedback = refining_viewports.contains(&id)
                && data.bindings.get(&self.tree).is_some_and(|b| {
                    b.element == id
                        && b.scopes == scrolling.scopes
                        && b.state.viewport == state.viewport
                        && b.state.bounds == state.bounds
                        && b.state.range[0] == state.range[0]
                        && b.state.offset[0] == state.offset[0]
                });
            data.bindings.insert(
                self.tree,
                Binding {
                    element: id,
                    scopes: scrolling.scopes.clone(),
                    life: Rc::downgrade(&scrolling.life),
                    commands: Rc::downgrade(&scrolling.commands),
                    state,
                },
            );
            data.readers
                .retain(|m| m.runtime != runtime.inner.id || runtime.inner.mount_live(*m));
            if changed {
                for &reader in &data.readers {
                    if scrolling.scopes.contains(&reader) {
                        runtime.inner.invalidate_mount(reader);
                        invalidated |= !measurement_feedback;
                    }
                }
            }
        }
        scrolling.revision = Some(revision);
        Ok(invalidated)
    }
    pub(crate) fn apply_scroll_commands(&mut self) -> Result<bool, UiError> {
        let commands = self
            .scrolling
            .as_ref()
            .map(|s| std::mem::take(&mut *s.commands.borrow_mut()))
            .unwrap_or_default();
        let mut changed = false;
        for c in commands {
            changed |= self.apply_scroll_command(c.target, c.offset, c.mode);
        }
        if changed {
            self.refresh_geometry()?;
        }
        Ok(changed)
    }
    pub(crate) fn set_scroll_offset(
        &mut self,
        target: ElementId,
        offset: [f32; 2],
    ) -> Result<bool, UiError> {
        let Some(n) = self.nodes.get_mut(&target) else {
            return Ok(false);
        };
        let next = [
            offset[0].clamp(0., n.scroll_range[0]),
            offset[1].clamp(0., n.scroll_range[1]),
        ];
        if next == n.scroll_offset {
            return Ok(false);
        }
        n.scroll_offset = next;
        self.refresh_geometry()?;
        Ok(true)
    }
}
