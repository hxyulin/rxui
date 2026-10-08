use super::*;
use crate::{DismissEvent, DismissReason, OverlayAnchor, PopoverPlacement, overlay::Properties};
#[derive(Default)]
pub(super) struct State {
    nodes: IdSet<ElementId>,
    pub roots: Vec<ElementId>,
    opened: Vec<Opened>,
    unavailable: IdSet<ElementId>,
    pub before_focus: Option<ElementId>,
}
struct Opened {
    id: ElementId,
    previous: Option<ElementId>,
    had_focus: bool,
}
impl State {
    /// Drops a removed node, so a preparation that fails before overlays are collected
    /// again leaves no root pointing at a missing node.
    pub(super) fn forget(&mut self, id: ElementId) {
        if self.nodes.remove(&id) {
            self.roots.retain(|root| *root != id);
            self.unavailable.remove(&id);
        }
    }
}
impl<T: View> Ui<T> {
    pub(super) fn snapshot_overlay_focus(&mut self) {
        let Some(state) = &mut self.overlays else {
            return;
        };
        for open in &mut state.opened {
            open.had_focus = self.focused.is_some_and(|focus| {
                let mut current = Some(focus);
                while let Some(id) = current {
                    if id == open.id {
                        return true;
                    }
                    current = self.nodes.get(&id).and_then(|n| n.parent);
                }
                false
            });
        }
    }
    pub(super) fn register_overlay_node(&mut self, id: ElementId) {
        if self.nodes[&id]
            .element
            .input
            .as_ref()
            .is_some_and(|p| p.overlay.is_some() || p.anchor.is_some())
        {
            self.overlays
                .get_or_insert_with(Default::default)
                .nodes
                .insert(id);
        }
    }
    pub(super) fn overlay_pending(&self) -> bool {
        self.overlays.as_ref().is_some_and(|s| {
            s.roots.iter().any(|id| {
                self.nodes.get(id).is_some_and(|n| !n.visible)
                    && !s.unavailable.contains(id)
                    && matches!(
                        self.overlay_properties(*id).and_then(|p| p.anchor.as_ref()),
                        Some(OverlayAnchor::Element(_))
                    )
            })
        })
    }
    fn overlay_properties(&self, id: ElementId) -> Option<&Properties> {
        self.nodes
            .get(&id)?
            .element
            .input
            .as_ref()?
            .overlay
            .as_deref()
    }
    pub(crate) fn is_overlay(&self, id: ElementId) -> bool {
        self.overlay_properties(id).is_some()
    }
    pub(super) fn in_modal_layer(&self, mut id: ElementId) -> bool {
        loop {
            if self.overlay_properties(id).is_some_and(|p| p.modal) {
                return true;
            }
            let Some(parent) = self.nodes[&id].parent else {
                return false;
            };
            id = parent;
        }
    }
    pub(super) fn active_modal(&self) -> Option<ElementId> {
        self.overlays
            .as_ref()?
            .roots
            .iter()
            .rev()
            .copied()
            .find(|id| {
                self.nodes.get(id).is_some_and(|n| n.visible && !n.inert)
                    && self.overlay_properties(*id).is_some_and(|p| p.modal)
            })
    }
    pub(super) fn modal_allows(&self, id: ElementId) -> bool {
        self.active_modal()
            .is_none_or(|modal| self.within(id, modal))
    }
    pub(super) fn overlay_semantic_allows(&self, id: ElementId) -> bool {
        self.active_modal()
            .is_none_or(|modal| self.within(id, modal) || self.within(modal, id))
    }
    fn anchor_node(&self, handle: &crate::AnchorHandle) -> Option<ElementId> {
        self.order.iter().copied().find(|id| {
            self.nodes[id]
                .element
                .input
                .as_ref()
                .and_then(|p| p.anchor.as_ref())
                .is_some_and(|h| h.id() == handle.id())
        })
    }
    /// Visible anchor border bounds in this prepared Ui only. A completely clipped,
    /// hidden, inert, missing or foreign-window anchor has no current geometry.
    pub fn anchor_bounds(&self, handle: &crate::AnchorHandle) -> Option<Bounds> {
        self.is_prepared()
            .then(|| self.anchor_geometry(handle))
            .flatten()
    }
    fn anchor_geometry(&self, handle: &crate::AnchorHandle) -> Option<Bounds> {
        let node = &self.nodes[&self.anchor_node(handle)?];
        let clipped = node.bounds.intersection(node.clip_bounds);
        (node.visible && !node.inert && clipped.width > 0. && clipped.height > 0.)
            .then_some(node.bounds)
    }
    pub(super) fn collect_overlays(
        &mut self,
        before_focus: Option<ElementId>,
        viewport: [f32; 2],
    ) -> Result<(), UiError> {
        let Some(state) = &mut self.overlays else {
            return Ok(());
        };
        state.nodes.retain(|id| {
            self.nodes.get(id).is_some_and(|n| {
                n.element
                    .input
                    .as_ref()
                    .is_some_and(|p| p.overlay.is_some() || p.anchor.is_some())
            })
        });
        let mut anchors = HashSet::new();
        let mut roots = Vec::new();
        for id in &self.order {
            if !self.overlays.as_ref().unwrap().nodes.contains(id) {
                continue;
            }
            if let Some(handle) = self.nodes[id]
                .element
                .input
                .as_ref()
                .and_then(|p| p.anchor.as_ref())
                && !anchors.insert(handle.id())
            {
                return Err(UiError::DuplicateAnchorHandle);
            }
            if self.is_overlay(*id) {
                roots.push(*id);
            }
        }
        if roots.is_empty() && self.overlays.is_none() {
            return Ok(());
        }
        let state = self.overlays.get_or_insert_with(Default::default);
        state.before_focus = before_focus;
        state.roots = roots;
        for root in state.roots.clone() {
            let margin = self.overlay_properties(root).unwrap().margin;
            let surface = self.nodes[&root].children[0];
            let mut style = self.nodes[&surface].element.style.clone();
            style.max_size = Size {
                width: length((viewport[0] - 2. * margin).max(0.)),
                height: length((viewport[1] - 2. * margin).max(0.)),
            };
            style.align_self = Some(AlignSelf::START);
            style.justify_self = Some(AlignSelf::START);
            if self.taffy.style(self.nodes[&surface].layout)? != &style {
                self.taffy.set_style(self.nodes[&surface].layout, style)?;
            }
        }
        Ok(())
    }
    pub(super) fn refresh_overlay_bounds(&mut self, viewport: [f32; 2]) -> Result<(), UiError> {
        let roots = self
            .overlays
            .as_ref()
            .map(|s| s.roots.clone())
            .unwrap_or_default();
        let clip = Bounds {
            x: 0.,
            y: 0.,
            width: viewport[0],
            height: viewport[1],
        };
        for root in roots {
            let props = self.overlay_properties(root).unwrap().clone();
            let mut visible = self.nodes[&root]
                .parent
                .is_none_or(|p| self.nodes[&p].visible && !self.nodes[&p].inert);
            let anchor = match &props.anchor {
                None => None,
                Some(OverlayAnchor::Point(p)) => Some(Bounds {
                    x: p[0],
                    y: p[1],
                    width: 0.,
                    height: 0.,
                }),
                Some(OverlayAnchor::Element(h)) => {
                    if self.anchor_node(h).is_some_and(|id| self.within(id, root)) {
                        return Err(UiError::InvalidOverlay);
                    }
                    let bounds = self.anchor_geometry(h);
                    visible &= bounds.is_some();
                    bounds
                }
            };
            self.update_bounds(root, [0., 0.], visible, clip)?;
            let surface = self.nodes[&root].children[0];
            let size = self.nodes[&surface].bounds;
            let position = place(anchor, [size.width, size.height], viewport, &props);
            let layout = self.taffy.layout(self.nodes[&surface].layout)?;
            self.update_bounds(
                surface,
                [
                    position[0] - layout.location.x,
                    position[1] - layout.location.y,
                ],
                visible,
                clip,
            )?;
        }
        Ok(())
    }
    pub(super) fn dismiss_unavailable_overlays(
        &mut self,
        runtime: &mut Runtime,
    ) -> Result<(), UiError> {
        let Some(state) = &mut self.overlays else {
            return Ok(());
        };
        state
            .unavailable
            .retain(|id| state.roots.contains(id) && !self.nodes[id].visible);
        let mut notify = Vec::new();
        for id in &state.roots {
            let node = &self.nodes[id];
            if !node.visible
                && matches!(
                    node.element
                        .input
                        .as_ref()
                        .and_then(|p| p.overlay.as_ref())
                        .and_then(|p| p.anchor.as_ref()),
                    Some(OverlayAnchor::Element(_))
                )
                && state.unavailable.insert(*id)
            {
                notify.push(*id);
            }
        }
        for id in notify {
            self.dismiss_overlay(runtime, id, DismissReason::AnchorUnavailable)?;
        }
        Ok(())
    }
    pub(super) fn sync_overlay_focus(&mut self) {
        let Some(mut state) = self.overlays.take() else {
            return;
        };
        let active: Vec<_> = state
            .roots
            .iter()
            .copied()
            .filter(|id| self.nodes[id].visible && !self.nodes[id].inert)
            .collect();
        // Put state back while resolving eligibility, so active modal confinement applies.
        let mut opened = std::mem::take(&mut state.opened);
        let mut fallback = state.before_focus;
        self.overlays = Some(state);
        for i in (0..opened.len()).rev() {
            if !active.contains(&opened[i].id) {
                let old = opened.remove(i);
                if old.had_focus || self.focused.is_none() {
                    fallback = old.previous.filter(|id| self.enabled_unconfined(*id));
                    self.change_focus(fallback.filter(|id| self.enabled(*id)));
                }
            }
        }
        for id in active {
            if opened.iter().any(|o| o.id == id) {
                continue;
            }
            let previous = self
                .focused
                .or(fallback)
                .filter(|focus| self.enabled_unconfined(*focus) && !self.within(*focus, id))
                .or_else(
                    || match self.overlay_properties(id).and_then(|p| p.anchor.as_ref()) {
                        Some(OverlayAnchor::Element(handle)) => self
                            .anchor_node(handle)
                            .filter(|anchor| self.enabled_unconfined(*anchor)),
                        _ => None,
                    },
                );
            opened.push(Opened {
                id,
                previous,
                had_focus: false,
            });
            let props = self.overlay_properties(id).unwrap();
            if self.modal_allows(id) && (props.autofocus || props.modal) {
                self.change_focus(self.group_target(self.nodes[&id].children[0], false, false));
            }
        }
        if let Some(modal) = self.active_modal()
            && self.focused.is_none_or(|id| !self.within(id, modal))
        {
            self.change_focus(self.group_target(self.nodes[&modal].children[0], false, false));
        }
        self.overlays.as_mut().unwrap().opened = opened;
    }
    fn top_overlay(&self, pointer: bool) -> Option<ElementId> {
        self.overlays
            .as_ref()?
            .roots
            .iter()
            .rev()
            .copied()
            .find(|id| {
                self.nodes[id].visible
                    && !self.nodes[id].inert
                    && self.modal_allows(*id)
                    && (!pointer || self.pointer_allowed(*id))
            })
    }
    fn dismiss_overlay(
        &self,
        runtime: &mut Runtime,
        id: ElementId,
        reason: DismissReason,
    ) -> Result<bool, UiError> {
        let Some(listener) = self.overlay_properties(id).and_then(|p| p.dismiss.as_ref()) else {
            return Ok(false);
        };
        Ok(
            runtime.update(|cx| listener.dispatch(&DismissEvent { reason }, cx))?
                == Dispatch::Handled,
        )
    }
    pub(super) fn overlay_pointer(
        &mut self,
        runtime: &mut Runtime,
        kind: usize,
        prevented: bool,
    ) -> Result<crate::InputResult, UiError> {
        if kind != 0 || prevented {
            return Ok(Default::default());
        }
        let Some(root) = self.top_overlay(true) else {
            return Ok(Default::default());
        };
        let surface = self.nodes[&root].children[0];
        if !self.nodes[&surface].bounds.contains(self.input.position) {
            self.cancel_capture(crate::PointerCancelReason::Host);
            self.pressed = None;
            return Ok(crate::InputResult {
                changed: self.dismiss_overlay(runtime, root, DismissReason::OutsidePointer)?,
                default_prevented: true,
            });
        }
        Ok(Default::default())
    }
    pub(super) fn overlay_key(
        &mut self,
        runtime: &mut Runtime,
        event: &crate::KeyEvent,
    ) -> Result<crate::InputResult, UiError> {
        if event.key == crate::KeyboardKey::Escape
            && self.captured_pointer().is_none()
            && let Some(root) = self.top_overlay(false)
        {
            return Ok(crate::InputResult {
                changed: self.dismiss_overlay(runtime, root, DismissReason::Escape)?,
                default_prevented: true,
            });
        }
        let mut current = self.focused;
        while let Some(id) = current {
            if self.nodes[&id]
                .element
                .input
                .as_ref()
                .is_some_and(|p| p.menu)
            {
                let items: Vec<_> = self
                    .order
                    .iter()
                    .copied()
                    .filter(|child| {
                        self.within(*child, id)
                            && self.enabled(*child)
                            && self.nodes[child]
                                .element
                                .semantics
                                .as_ref()
                                .and_then(|p| p.role)
                                == Some(crate::SemanticRole::MenuItem)
                    })
                    .collect();
                if items.is_empty() {
                    break;
                }
                let index = items
                    .iter()
                    .position(|i| Some(*i) == self.focused)
                    .unwrap_or(0);
                let next = match event.key {
                    crate::KeyboardKey::ArrowDown => (index + 1) % items.len(),
                    crate::KeyboardKey::ArrowUp => (index + items.len() - 1) % items.len(),
                    crate::KeyboardKey::Home => 0,
                    crate::KeyboardKey::End => items.len() - 1,
                    _ => break,
                };
                if event.modifiers != Default::default() {
                    break;
                }
                self.change_focus(Some(items[next]));
                self.reveal(items[next]);
                return Ok(crate::InputResult {
                    changed: true,
                    default_prevented: true,
                });
            }
            current = self.nodes[&id].parent;
        }
        Ok(Default::default())
    }
}
fn place(anchor: Option<Bounds>, size: [f32; 2], viewport: [f32; 2], p: &Properties) -> [f32; 2] {
    let margin = [
        p.margin.min(viewport[0] * 0.5),
        p.margin.min(viewport[1] * 0.5),
    ];
    let mut point = [(viewport[0] - size[0]) * 0.5, (viewport[1] - size[1]) * 0.5];
    if let Some(a) = anchor {
        let (axis, forward) = match p.placement {
            PopoverPlacement::BottomStart | PopoverPlacement::BottomEnd => (1, true),
            PopoverPlacement::TopStart | PopoverPlacement::TopEnd => (1, false),
            PopoverPlacement::RightStart => (0, true),
            PopoverPlacement::LeftStart => (0, false),
        };
        point = [a.x, a.y];
        if matches!(
            p.placement,
            PopoverPlacement::BottomEnd | PopoverPlacement::TopEnd
        ) {
            point[0] = a.x + a.width - size[0];
        }
        let start = [a.x, a.y][axis];
        let end = start + [a.width, a.height][axis];
        let ahead = end + p.gap;
        let behind = start - p.gap - size[axis];
        let fits = |x: f32| x >= margin[axis] && x + size[axis] <= viewport[axis] - margin[axis];
        let preferred = if forward { ahead } else { behind };
        let opposite = if forward { behind } else { ahead };
        point[axis] = if !fits(preferred) && fits(opposite) {
            opposite
        } else {
            preferred
        };
    }
    for axis in 0..2 {
        point[axis] = point[axis].clamp(
            margin[axis],
            (viewport[axis] - margin[axis] - size[axis]).max(margin[axis]),
        );
    }
    point
}
