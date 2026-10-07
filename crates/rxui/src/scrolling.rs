//! Placement-scoped scroll references. Sharing a handle never shares offsets.
use crate::{AppContext, Bounds, ElementId, MountId, ReadContext, Runtime, Ui, UiError, View};
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
    /// Visible content-box dimensions.
    pub viewport: [f32; 2],
    /// Absolute logical content box.
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
    /// Queues absolute logical offsets in the current listener's placement.
    pub fn scroll_to(&self, cx: &mut AppContext<'_>, offset: [f32; 2]) -> Result<(), ScrollError> {
        self.placement(cx)?.scroll_to(cx, offset)
    }
    /// Queues relative motion; clamping occurs when applied to current content geometry.
    pub fn scroll_by(&self, cx: &mut AppContext<'_>, delta: [f32; 2]) -> Result<(), ScrollError> {
        self.placement(cx)?.scroll_by(cx, delta)
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
    pub fn scroll_to(&self, cx: &mut AppContext<'_>, offset: [f32; 2]) -> Result<(), ScrollError> {
        self.command(cx, offset, false)
    }
    /// Queue relative motion through an explicit source.
    pub fn scroll_by(&self, cx: &mut AppContext<'_>, delta: [f32; 2]) -> Result<(), ScrollError> {
        self.command(cx, delta, true)
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
        let state = self.state()?;
        let start = (index as f64 * f64::from(row_height)) as f32;
        let y = if start < state.offset[1] || row_height > state.viewport[1] {
            start
        } else if end as f32 > state.offset[1] + state.viewport[1] {
            end as f32 - state.viewport[1]
        } else {
            state.offset[1]
        };
        self.scroll_to(cx, [state.offset[0], y])
    }
    fn command(
        &self,
        cx: &mut AppContext<'_>,
        offset: [f32; 2],
        relative: bool,
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
                relative,
            });
        Ok(())
    }
}
pub(crate) struct Command {
    pub target: ElementId,
    pub offset: [f32; 2],
    pub relative: bool,
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
            let state = ScrollState {
                offset: n.scroll_offset,
                range: n.scroll_range,
                viewport: [n.content_bounds.width, n.content_bounds.height],
                bounds: n.content_bounds,
            };
            let mut data = handle.0.borrow_mut();
            let changed = data.bindings.get(&self.tree).is_none_or(|b| {
                b.element != id || b.state != state || b.scopes != scrolling.scopes
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
                        invalidated = true;
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
            if let Some(n) = self.nodes.get_mut(&c.target) {
                for axis in 0..2 {
                    let next = (c.offset[axis]
                        + if c.relative {
                            n.scroll_offset[axis]
                        } else {
                            0.
                        })
                    .clamp(0., n.scroll_range[axis]);
                    changed |= next != n.scroll_offset[axis];
                    n.scroll_offset[axis] = next;
                }
            }
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
