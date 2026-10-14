//! Retained motion for one placement: implicit paint transitions, the focus ring
//! fade and overlay enter/exit. Everything is advanced once per `Ui::prepare` from
//! the runtime's frame time. While nothing moves, preparation only compares four
//! interaction targets and the open overlay roots, and nothing requests a frame.
use super::*;
use crate::{
    Transition,
    animation::{Interpolate, Timing},
};
use std::time::Duration;

/// Motion bookkeeping retained by a `Ui`; `active` is allocated only while moving.
#[derive(Default)]
pub(super) struct Motion {
    /// Whether transitions animate this preparation, read from the runtime clock.
    animates: bool,
    /// Whether this is the first preparation since `Runtime::begin_frame`, so the
    /// frame time is current. Moves created otherwise start at the next frame.
    in_frame: bool,
    /// Frame time of the current preparation.
    now: Duration,
    /// Interaction targets at the end of the previous preparation.
    hovered: Option<ElementId>,
    pressed: Option<ElementId>,
    captured: Option<ElementId>,
    /// Focused element with a visible ring.
    focused: Option<ElementId>,
    /// Overlay roots open at the end of the previous preparation, with the side
    /// they slide from.
    overlays: Vec<(ElementId, [f32; 2])>,
    /// Advances on every frame with layer motion, invalidating composition plans.
    revision: u64,
    /// Paint displayed before a description or style change, compared with the new
    /// target once preparation completes.
    pending: IdMap<ElementId, ResolvedPaint>,
    /// Overlay subtree being moved out of the tree by `Ui::remove`.
    exiting: Option<Box<Ghost>>,
    active: Option<Box<Active>>,
}
struct Active {
    paints: IdMap<ElementId, PaintMotion>,
    focus: Option<(ElementId, Progress)>,
    overlays: IdMap<ElementId, OverlayMotion>,
    ghosts: Vec<Ghost>,
    /// Paint order including ghosts; empty without ghosts.
    order: Vec<ElementId>,
    _frame: FrameRequest,
}
/// Counts this placement in `Runtime::animation_frame_requested` while alive.
struct FrameRequest(std::rc::Weak<crate::runtime::RuntimeInner>);
impl FrameRequest {
    fn new(runtime: &std::rc::Weak<crate::runtime::RuntimeInner>) -> Self {
        if let Some(runtime) = runtime.upgrade() {
            runtime.animating.set(runtime.animating.get() + 1);
        }
        Self(runtime.clone())
    }
}
impl Drop for FrameRequest {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.upgrade() {
            runtime
                .animating
                .set(runtime.animating.get().saturating_sub(1));
        }
    }
}
/// A move that starts at the first frame after it was created.
#[derive(Clone, Copy)]
struct Progress {
    start: Option<Duration>,
    timing: Timing,
}
impl Progress {
    fn new(timing: Timing) -> Self {
        Self {
            start: None,
            timing,
        }
    }
    /// Eased progress and whether it has ended at `now`.
    fn at(&self, now: Duration) -> (f32, bool) {
        match self.start {
            None => (0., false),
            Some(start) => self.timing.progress(now.saturating_sub(start), 0.),
        }
    }
}
struct PaintMotion {
    from: ResolvedPaint,
    progress: Progress,
}
struct OverlayMotion {
    /// Opacity factor when entering began; nonzero when reopened while exiting.
    from: f32,
    /// Displacement when entering began, returning to zero.
    #[cfg_attr(not(feature = "rendering"), allow(dead_code))]
    offset: [f32; 2],
    progress: Progress,
}
impl OverlayMotion {
    fn opacity(&self, now: Duration) -> f32 {
        let (t, _) = self.progress.at(now);
        (self.from + (1. - self.from) * t).clamp(0., 1.)
    }
}
/// The retained visual of a closed overlay while it fades out. Its nodes left the
/// tree, so focus, input, semantics and listeners already ignore them.
struct Ghost {
    root: ElementId,
    parent: Option<ElementId>,
    key: Option<Key>,
    order: Vec<ElementId>,
    paints: IdMap<ElementId, ResolvedPaint>,
    nodes: IdMap<ElementId, Node>,
    /// Opacity factor when exiting began.
    from: f32,
    #[cfg_attr(not(feature = "rendering"), allow(dead_code))]
    offset: [f32; 2],
    progress: Progress,
}
impl Ghost {
    fn opacity(&self, now: Duration) -> f32 {
        let (t, _) = self.progress.at(now);
        (self.from * (1. - t)).clamp(0., 1.)
    }
    #[cfg_attr(not(feature = "rendering"), allow(dead_code))]
    fn offset(&self, now: Duration) -> [f32; 2] {
        let (t, _) = self.progress.at(now);
        [self.offset[0] * t, self.offset[1] * t]
    }
}
/// The element's explicit transition, kept with its state styles.
fn transition(element: &Element) -> Option<Transition> {
    element.states.as_ref().and_then(|s| s.transition)
}
impl<T: View> Ui<T> {
    /// Whether retained motion is in flight: a paint transition, a focus ring fade
    /// or an overlay entering or exiting. Hosts keep scheduling frames, calling
    /// `Runtime::begin_frame` and preparing, while this holds; it becomes false
    /// once everything has settled. `Runtime::animation_frame_requested` includes it.
    pub fn is_animating(&self) -> bool {
        self.motion.active.is_some()
    }
    /// Reads the runtime clock before preparation captures any paint.
    pub(super) fn begin_motion(&mut self, runtime: &Runtime) {
        let clock = runtime.inner.motion.get();
        self.motion.animates = clock.animates();
        self.motion.in_frame = clock.fresh();
        self.motion.now = runtime.inner.frames.borrow().1;
        // A preparation that failed while removing an overlay leaves a partial ghost.
        self.motion.exiting = None;
    }
    fn active(&mut self) -> &mut Active {
        let runtime = &self.runtime;
        self.motion.active.get_or_insert_with(|| {
            Box::new(Active {
                paints: IdMap::default(),
                focus: None,
                overlays: IdMap::default(),
                ghosts: Vec::new(),
                order: Vec::new(),
                _frame: FrameRequest::new(runtime),
            })
        })
    }
    /// Timing of `node`'s paint transitions; theme changes animate every element
    /// that does not opt out.
    fn paint_timing(node: &Node, theme_change: bool) -> Option<Timing> {
        let state = node.resolved_theme.sizes().motion.state;
        match transition(&node.element) {
            Some(transition) => transition.timing(state),
            None if theme_change || node.state_paints.is_some() => Transition::Theme.timing(state),
            None => None,
        }
    }
    /// Whether `node` was painted in the previous frame: visible and not clipped
    /// away, like rows scrolled out of view. Others change without a transition.
    fn on_screen(node: &Node) -> bool {
        let b = node.bounds.intersection(node.clip_bounds);
        node.visible && b.width > 0. && b.height > 0.
    }
    fn disabled(&self, id: ElementId, node: &Node) -> bool {
        self.range_unavailable(id)
            || matches!(
                node.element.kind,
                ElementKind::Button { disabled: true, .. }
                    | ElementKind::TextInput { disabled: true, .. }
            )
    }
    /// The paint `id` displays now, including its own transition in flight.
    fn displayed_paint(&self, id: ElementId, node: &Node) -> ResolvedPaint {
        let target = self.state_paint(
            id,
            node,
            self.disabled(id, node),
            [self.hovered, self.pressed, self.captured_pointer()],
        );
        self.own_motion(id, target)
    }
    /// Blends `target` with the paint transition of `id` alone.
    fn own_motion(&self, id: ElementId, target: ResolvedPaint) -> ResolvedPaint {
        match self.motion.active.as_ref().and_then(|a| a.paints.get(&id)) {
            Some(m) => m
                .from
                .interpolate(&target, m.progress.at(self.motion.now).0),
            None => target,
        }
    }
    /// Records the paint `id` displays before its description or resolved style
    /// changes. Called before the change; compared after preparation.
    pub(super) fn capture_paint(&mut self, id: ElementId, theme: Option<&Theme>) {
        if !self.motion.animates || self.motion.pending.contains_key(&id) {
            return;
        }
        let node = &self.nodes[&id];
        if !Self::on_screen(node)
            || Self::paint_timing(node, theme.is_some_and(|t| *t != node.resolved_theme)).is_none()
        {
            return;
        }
        let paint = self.displayed_paint(id, node);
        self.motion.pending.insert(id, paint);
    }
    /// Blends `target` with the motion of `id` at the current frame time.
    pub(super) fn motion_paint(&self, id: ElementId, mut target: ResolvedPaint) -> ResolvedPaint {
        let Some(active) = &self.motion.active else {
            return target;
        };
        let now = self.motion.now;
        target = self.own_motion(id, target);
        if let Some(owner) = self.nodes.get(&id).and_then(|n| {
            n.control_color
                .then_some(n.button_owner)
                .flatten()
                .filter(|owner| active.paints.contains_key(owner))
        }) {
            // Composed button content follows its owner's moving foreground.
            let owner = &active.paints[&owner];
            target.color = owner
                .from
                .color
                .interpolate(&target.color, owner.progress.at(now).0);
        }
        if let Some((focus, progress)) = &active.focus
            && *focus == id
        {
            target.focus_color[3] *= progress.at(now).0.clamp(0., 1.);
        }
        target
    }
    /// Opacity factor of an entering overlay root, or one.
    pub(super) fn overlay_opacity(&self, id: ElementId) -> f32 {
        self.motion
            .active
            .as_ref()
            .and_then(|a| a.overlays.get(&id))
            .map_or(1., |m| m.opacity(self.motion.now))
    }
    /// Whether a moving layer needs offscreen composition.
    pub(super) fn motion_layers(&self) -> bool {
        self.motion
            .active
            .as_ref()
            .is_some_and(|a| !a.overlays.is_empty() || !a.ghosts.is_empty())
    }
    #[cfg_attr(not(feature = "rendering"), allow(dead_code))]
    pub(super) fn motion_revision(&self) -> u64 {
        self.motion.revision
    }
    /// Whether a paint transition of `id` started from a shadow.
    #[cfg_attr(not(feature = "rendering"), allow(dead_code))]
    pub(super) fn motion_shadow(&self, id: ElementId) -> bool {
        self.motion
            .active
            .as_ref()
            .and_then(|a| a.paints.get(&id))
            .is_some_and(|m| m.from.shadow.is_some())
    }
    /// Paint order including exiting overlays, which paint above ordinary content
    /// and below open overlays.
    pub(super) fn motion_order(&self) -> Option<&[ElementId]> {
        self.motion
            .active
            .as_ref()
            .filter(|a| !a.ghosts.is_empty())
            .map(|a| a.order.as_slice())
    }
    /// A node retained only for an exiting overlay's visual.
    pub(super) fn ghost_node(&self, id: ElementId) -> Option<&Node> {
        self.motion
            .active
            .as_ref()?
            .ghosts
            .iter()
            .find_map(|g| g.nodes.get(&id))
    }
    /// Composition parent and opacity of a ghost node; roots fade out.
    #[cfg_attr(not(feature = "rendering"), allow(dead_code))]
    pub(super) fn ghost_composition_node(&self, id: ElementId) -> Option<(Option<ElementId>, f32)> {
        let active = self.motion.active.as_ref()?;
        let ghost = active.ghosts.iter().find(|g| g.nodes.contains_key(&id))?;
        let node = ghost.nodes.get(&id).filter(|n| n.visible)?;
        Some(if id == ghost.root {
            (None, node.element.opacity * ghost.opacity(self.motion.now))
        } else if node
            .element
            .input
            .as_ref()
            .is_some_and(|p| p.overlay.is_some())
        {
            // Nested overlays paint after the root's subtree and fade with it.
            (Some(ghost.root), node.element.opacity)
        } else {
            (node.parent, node.element.opacity)
        })
    }
    /// Displacement of an entering or exiting overlay layer.
    #[cfg_attr(not(feature = "rendering"), allow(dead_code))]
    pub(crate) fn layer_offset(&self, id: ElementId) -> [f32; 2] {
        let Some(active) = &self.motion.active else {
            return [0.; 2];
        };
        let now = self.motion.now;
        if let Some(m) = active.overlays.get(&id) {
            let (t, _) = m.progress.at(now);
            return [m.offset[0] * (1. - t), m.offset[1] * (1. - t)];
        }
        active
            .ghosts
            .iter()
            .find(|g| g.root == id)
            .map_or([0.; 2], |g| g.offset(now))
    }
    /// Snapshot of a ghost node with no interaction state.
    pub(super) fn ghost_element(&self, id: ElementId) -> Option<ElementInfo<'_>> {
        let active = self.motion.active.as_ref()?;
        let ghost = active.ghosts.iter().find(|g| g.nodes.contains_key(&id))?;
        let node = ghost.nodes.get(&id).filter(|n| n.visible)?;
        let paint = ghost.paints.get(&id).copied().unwrap_or(node.paint);
        Some(ElementInfo {
            viewport_overlay: id == ghost.root,
            id,
            parent: node.parent,
            opacity: if id == ghost.root {
                node.element.opacity * ghost.opacity(self.motion.now)
            } else {
                node.element.opacity
            },
            key: node.element.key.as_ref(),
            kind: kind(&node.element),
            bounds: node.bounds,
            content_bounds: node.content_bounds,
            clip_bounds: node.clip_bounds,
            rounded_clip: node.rounded_clip,
            scroll_offset: node.scroll_offset,
            scroll_range: node.scroll_range,
            z_index: node.element.z_index,
            pointer_events: node.element.pointer_events,
            inert: true,
            text: node.displayed_text(),
            placeholder: node.placeholder(),
            image: if let ElementKind::Image(props) = &node.element.kind {
                let (destination, uv) =
                    crate::image::placement(node.content_bounds, node.image_size, props);
                Some(crate::ImageInfo {
                    source: &props.source,
                    destination,
                    uv,
                    tint: node.image_tint,
                    filter: props.filter,
                    alpha: props.alpha,
                })
            } else {
                None
            },
            editing: None,
            font_size: node.font_size,
            text_style: &node.text_style,
            text_revision: node.text_revision,
            color: paint.color,
            background: paint.background,
            paint,
            border: node.border,
            disabled: matches!(
                node.element.kind,
                ElementKind::Button { disabled: true, .. }
                    | ElementKind::TextInput { disabled: true, .. }
            ),
            hovered: false,
            pressed: false,
            focused: false,
            focus_visible: false,
            focusable: false,
            range: None,
        })
    }
    /// Starts retaining the visual of overlay root `id`, which is being removed.
    /// Returns whether `id` roots the ghost, so `retire_node` completes it.
    pub(super) fn begin_exit(&mut self, id: ElementId) -> bool {
        if !self.motion.animates || self.motion.exiting.is_some() || !self.is_overlay(id) {
            return false;
        }
        let node = &self.nodes[&id];
        let motion = node.resolved_theme.sizes().motion.clone();
        let Some(timing) = transition(&node.element)
            .unwrap_or_default()
            .timing(motion.overlay_exit)
        else {
            return false;
        };
        if !node.visible {
            return false;
        }
        // Ancestors may already be gone when a removed subtree contains the overlay.
        let order: Vec<_> = self
            .painting_order()
            .iter()
            .copied()
            .filter(|n| self.nodes.get(n).is_some_and(|n| n.visible) && self.within(*n, id))
            .collect();
        let paints = order.iter().map(|n| (*n, self.ghost_paint(*n))).collect();
        let offset = self
            .motion
            .overlays
            .iter()
            .find(|(root, _)| *root == id)
            .map_or([0.; 2], |(_, offset)| *offset);
        let from = self.overlay_opacity(id);
        if let Some(active) = &mut self.motion.active {
            active.overlays.remove(&id);
        }
        self.motion.exiting = Some(Box::new(Ghost {
            root: id,
            parent: node.parent,
            key: node.element.key.clone(),
            order,
            paints,
            nodes: IdMap::default(),
            from,
            offset,
            progress: Progress::new(timing),
        }));
        true
    }
    /// Paint of a node about to become part of a ghost, as last displayed.
    fn ghost_paint(&self, id: ElementId) -> ResolvedPaint {
        let node = &self.nodes[&id];
        let mut paint = self.displayed_paint(id, node);
        if let Some(owner) = node.button_owner.filter(|_| node.control_color)
            && let Some(owner_node) = self.nodes.get(&owner)
        {
            paint.color = self.displayed_paint(owner, owner_node).color;
        }
        paint
    }
    /// Drops a removed node, or keeps it for the overlay visual being retained.
    pub(super) fn retire_node(&mut self, id: ElementId, mut node: Node, root: bool) {
        let Some(ghost) = &mut self.motion.exiting else {
            return;
        };
        if ghost.paints.contains_key(&id) {
            // Release placement state; only what painting reads stays.
            node.mounted = None;
            node.editor = None;
            node.state_paints = None;
            node.button_owner = None;
            node.control_color = false;
            ghost.nodes.insert(id, node);
        }
        if root {
            let ghost = self.motion.exiting.take().unwrap();
            self.active().ghosts.push(*ghost);
        }
    }
    /// Direction an overlay slides from: away from its anchor, or none for modals.
    fn overlay_offset(&self, id: ElementId, distance: f32) -> [f32; 2] {
        let Some(props) = self
            .nodes
            .get(&id)
            .and_then(|n| n.element.input.as_ref())
            .and_then(|p| p.overlay.as_deref())
        else {
            return [0.; 2];
        };
        let anchor = match &props.anchor {
            None => return [0.; 2],
            Some(crate::OverlayAnchor::Point(p)) => [p[0], p[1]],
            Some(crate::OverlayAnchor::Element(handle)) => match self.anchor_bounds(handle) {
                Some(b) => [b.x + b.width * 0.5, b.y + b.height * 0.5],
                None => return [0.; 2],
            },
        };
        let Some(surface) = self.nodes[&id]
            .children
            .first()
            .map(|c| self.nodes[c].bounds)
        else {
            return [0.; 2];
        };
        let horizontal = matches!(
            props.placement,
            crate::PopoverPlacement::RightStart | crate::PopoverPlacement::LeftStart
        );
        if horizontal {
            let right = surface.x + surface.width * 0.5 >= anchor[0];
            [if right { -distance } else { distance }, 0.]
        } else {
            let below = surface.y + surface.height * 0.5 >= anchor[1];
            [0., if below { -distance } else { distance }]
        }
    }
    /// Starts transitions for changes since the previous preparation, then advances
    /// and retires motion at the current frame time. O(1) while idle.
    pub(super) fn finish_motion(&mut self, runtime: &Runtime) {
        let mut clock = runtime.inner.motion.get();
        clock.consumed = clock.frame;
        runtime.inner.motion.set(clock);
        let interaction = [
            self.hovered,
            self.pressed,
            self.captured_pointer(),
            self.visible_ring(),
        ];
        let previous = [
            self.motion.hovered,
            self.motion.pressed,
            self.motion.captured,
            self.motion.focused,
        ];
        [
            self.motion.hovered,
            self.motion.pressed,
            self.motion.captured,
            self.motion.focused,
        ] = interaction;
        let overlays_changed = match &self.overlays {
            Some(state) => {
                state.roots.len() != self.motion.overlays.len()
                    || state
                        .roots
                        .iter()
                        .zip(&self.motion.overlays)
                        .any(|(a, (b, _))| a != b)
            }
            None => !self.motion.overlays.is_empty(),
        };
        if !self.motion.animates {
            self.motion.pending.clear();
            if self.motion.active.take().is_some() {
                self.motion.revision += 1;
            }
            if overlays_changed {
                self.overlay_motion();
            }
            return;
        }
        if interaction != previous {
            self.interaction_motion(previous);
        }
        if !self.motion.pending.is_empty() {
            for (id, from) in std::mem::take(&mut self.motion.pending) {
                let Some(node) = self.nodes.get(&id) else {
                    continue;
                };
                let target = self.state_paint(
                    id,
                    node,
                    self.disabled(id, node),
                    [self.hovered, self.pressed, self.captured_pointer()],
                );
                let timing = Self::paint_timing(node, true);
                if let Some(timing) = timing
                    && target != from
                    && Self::on_screen(node)
                {
                    self.active().paints.insert(
                        id,
                        PaintMotion {
                            from,
                            progress: Progress::new(timing),
                        },
                    );
                }
            }
        }
        if overlays_changed {
            self.overlay_motion();
        }
        if self.motion.active.is_some() {
            self.advance_motion();
        }
    }
    /// Focused element whose ring is shown; the ring fades in when this changes,
    /// including on the first keyboard input after pointer focus.
    fn visible_ring(&self) -> Option<ElementId> {
        self.focused.filter(|&id| self.focus_visible(id))
    }
    /// Paint transitions for elements whose hover/pressed/focus state changed.
    fn interaction_motion(&mut self, previous: [Option<ElementId>; 4]) {
        let [hovered, pressed, captured, focused] = previous;
        let mut ids: Vec<ElementId> = [
            hovered,
            pressed,
            captured,
            self.hovered,
            self.pressed,
            self.captured_pointer(),
        ]
        .into_iter()
        .flatten()
        .collect();
        ids.sort_unstable_by_key(|id| (id.tree, id.serial));
        ids.dedup();
        for id in ids {
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            let Some(timing) = Self::paint_timing(node, false).filter(|_| Self::on_screen(node))
            else {
                continue;
            };
            let disabled = self.disabled(id, node);
            let old = self.state_paint(id, node, disabled, [hovered, pressed, captured]);
            let new = self.state_paint(
                id,
                node,
                disabled,
                [self.hovered, self.pressed, self.captured_pointer()],
            );
            if old != new {
                let from = self.own_motion(id, old);
                self.active().paints.insert(
                    id,
                    PaintMotion {
                        from,
                        progress: Progress::new(timing),
                    },
                );
            }
        }
        if focused != self.visible_ring()
            && let Some(id) = self.visible_ring()
            && let Some(node) = self.nodes.get(&id)
            && Self::on_screen(node)
            && let Some(timing) = Self::paint_timing(node, false)
        {
            self.active().focus = Some((id, Progress::new(timing)));
        }
    }
    /// Enter motion for newly opened overlay roots.
    fn overlay_motion(&mut self) {
        let roots = self
            .overlays
            .as_ref()
            .map_or_else(Vec::new, |s| s.roots.clone());
        let mut snapshot = Vec::with_capacity(roots.len());
        for &id in &roots {
            if let Some(known) = self.motion.overlays.iter().find(|(root, _)| *root == id) {
                snapshot.push(*known);
                continue;
            }
            let node = &self.nodes[&id];
            let motion = node.resolved_theme.sizes().motion.clone();
            let offset = self.overlay_offset(id, motion.overlay_offset);
            snapshot.push((id, offset));
            if !self.motion.animates || !node.visible {
                continue;
            }
            let Some(timing) = transition(&node.element)
                .unwrap_or_default()
                .timing(motion.overlay_enter)
            else {
                continue;
            };
            // Reopening while the previous visual still fades continues from there.
            let (parent, key) = (node.parent, node.element.key.clone());
            let mut from = 0.;
            if let Some(active) = &mut self.motion.active
                && let Some(index) = active
                    .ghosts
                    .iter()
                    .position(|g| g.parent == parent && g.key == key)
            {
                from = active.ghosts.remove(index).opacity(self.motion.now);
            }
            self.active().overlays.insert(
                id,
                OverlayMotion {
                    from,
                    offset,
                    progress: Progress::new(timing),
                },
            );
        }
        self.motion.overlays = snapshot;
    }
    /// Starts pending moves at this frame and drops finished ones.
    fn advance_motion(&mut self) {
        let (now, in_frame) = (self.motion.now, self.motion.in_frame);
        let nodes = &self.nodes;
        let active = self.motion.active.as_mut().unwrap();
        let start = |p: &mut Progress| {
            if in_frame {
                p.start.get_or_insert(now);
            }
            !p.at(now).1
        };
        active
            .paints
            .retain(|id, m| nodes.contains_key(id) && start(&mut m.progress));
        if let Some((id, progress)) = &mut active.focus
            && !(nodes.contains_key(id) && start(progress))
        {
            active.focus = None;
        }
        let layers = !active.overlays.is_empty() || !active.ghosts.is_empty();
        active
            .overlays
            .retain(|id, m| nodes.contains_key(id) && start(&mut m.progress));
        active.ghosts.retain_mut(|g| start(&mut g.progress));
        if layers {
            self.motion.revision += 1;
        }
        if active.paints.is_empty()
            && active.focus.is_none()
            && active.overlays.is_empty()
            && active.ghosts.is_empty()
        {
            self.motion.active = None;
            return;
        }
        if !active.ghosts.is_empty() {
            let mut order = std::mem::take(&mut active.order);
            order.clear();
            let base = self.paint_order.as_deref().unwrap_or(&self.order);
            // Above ordinary content, below overlays that are still open.
            let split = base
                .iter()
                .position(|id| self.overlays.as_ref().is_some_and(|s| s.roots.contains(id)))
                .unwrap_or(base.len());
            order.extend_from_slice(&base[..split]);
            let active = self.motion.active.as_mut().unwrap();
            for ghost in &active.ghosts {
                order.extend_from_slice(&ghost.order);
            }
            order.extend_from_slice(&base[split..]);
            active.order = order;
        }
    }
}
