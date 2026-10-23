//! Header drag defaults, opt-in surface transfers and geometric docking previews.
use super::*;
use crate::{
    DockDetachEvent, DockDropQuery, DockDropTarget, DockEvent, DockNodeId, DockSide, DockSurfaceId,
    DockTransferEvent, SplitPosition,
    dock_view::{Metadata, Properties},
    input::{Cursor, InputResult, PointerButton, PointerCancelReason},
};

/// Current proposed drop in logical window coordinates. Preview rectangles describe
/// the current destination pane; final allocation can change after source collapse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockDropPreview {
    /// Checked model operation proposed on release.
    pub target: DockDropTarget,
    /// Visible destination area, or a narrow header insertion marker.
    pub bounds: Bounds,
    /// True for an insertion marker, false for a center/edge pane highlight.
    pub insertion: bool,
}
/// Borrowed snapshot of an active, placement-local drag. No snapshot exists until
/// the movement threshold is crossed. Custom painters can use it for feedback.
#[derive(Debug)]
pub struct DockDragInfo<'a> {
    /// Original tab group.
    pub source: DockNodeId,
    /// Panel being dragged.
    pub panel: &'a Key,
    /// Current logical pointer position.
    pub position: [f32; 2],
    /// Current destination, absent outside eligible groups/over blocked content.
    pub preview: Option<DockDropPreview>,
}
#[derive(Clone)]
pub(super) struct Drag {
    root: ElementId,
    header: ElementId,
    group: DockNodeId,
    panel: Key,
    surface: Option<DockSurfaceId>,
    scope: Option<Key>,
    press: [f32; 2],
    active: bool,
    external_release: bool,
    pub(super) inside: bool,
    pub(super) preview: Option<DockDropPreview>,
}
/// Owned source description for a custom host coordinating separate `Ui`s.
/// Obtain a preview snapshot with [`Ui::dock_drag_session`], then take the
/// release token with [`Ui::take_dock_drag_session`]. The host chooses the actual
/// native destination and converts its pointer position into destination logical
/// coordinates. This type does not infer desktop positions or window stacking.
pub struct DockDragSession {
    drag: Drag,
    source: Properties,
    released: bool,
}
impl DockDragSession {
    /// Explicit source workspace surface.
    pub fn source_surface(&self) -> DockSurfaceId {
        self.source.surface.unwrap()
    }
    /// Original source group, revalidated by application transfer acceptance.
    pub fn source_group(&self) -> DockNodeId {
        self.drag.group
    }
    /// Application panel identity.
    pub fn panel(&self) -> &Key {
        &self.drag.panel
    }
    /// Explicit transfer scope shared by participating surfaces.
    pub fn transfer_scope(&self) -> &Key {
        self.source.transfer_scope.as_ref().unwrap()
    }
}
impl<T: View> Ui<T> {
    fn dock_metadata(&self, id: ElementId) -> Option<&Metadata> {
        self.nodes.get(&id)?.element.input.as_ref()?.dock.as_deref()
    }
    fn dock_location(&self, mut id: ElementId) -> Option<(ElementId, ElementId, DockNodeId)> {
        let mut group = None;
        loop {
            if self.is_overlay(id) {
                return None;
            }
            match self.dock_metadata(id) {
                Some(Metadata::Group(key)) if group.is_none() => group = Some((id, *key)),
                Some(Metadata::Root(_)) => {
                    let (element, key) = group?;
                    return Some((id, element, key));
                }
                _ => {}
            }
            id = self.nodes.get(&id)?.parent?;
        }
    }
    fn dock_config(&self, root: ElementId) -> Option<&Properties> {
        match self.dock_metadata(root)? {
            Metadata::Root(p) => Some(p),
            _ => None,
        }
    }
    fn dock_header(&self, header: ElementId) -> Option<(ElementId, DockNodeId, Key)> {
        let Some(crate::tabs::Properties::Header { key, .. }) = self.tab_properties(header) else {
            return None;
        };
        let (root, element, group) = self.dock_location(header)?;
        // Nested ordinary tabs remain independent controls. Only headers of the
        // directly composed dock tab group can begin a docking gesture.
        let mut current = self.nodes[&header].parent;
        while let Some(id) = current {
            if matches!(
                self.tab_properties(id),
                Some(crate::tabs::Properties::Root { .. })
            ) {
                if id != element {
                    return None;
                }
                break;
            }
            current = self.nodes[&id].parent;
        }
        Some((root, group, key.clone()))
    }
    fn dock_source(&self, header: ElementId) -> Option<(ElementId, DockNodeId, Key)> {
        let (root, group, key) = self.dock_header(header)?;
        let config = self.dock_config(root)?;
        (config.draggable
            && (config.listener.is_some()
                || (config.surface.is_some()
                    && (config.detach.is_some()
                        || (config.transfer_scope.is_some() && config.transfer.is_some()))))
            && self.input_available(header)
            && self.input_available(root))
        .then_some((root, group, key))
    }
    fn dock_context(
        &mut self,
        runtime: &mut Runtime,
        header: ElementId,
        position: [f32; 2],
    ) -> Result<InputResult, UiError> {
        let Some((root, group, panel)) = self.dock_header(header) else {
            return Ok(Default::default());
        };
        if !self.input_available(header) {
            return Ok(Default::default());
        }
        let Some(listener) = self.dock_config(root).and_then(|p| p.context_menu.clone()) else {
            return Ok(Default::default());
        };
        self.change_focus(Some(header));
        let event = crate::DockContextEvent {
            group,
            panel,
            position,
        };
        let changed = runtime.update(|cx| listener.dispatch(&event, cx))? == Dispatch::Handled;
        Ok(InputResult {
            changed,
            default_prevented: true,
        })
    }
    pub(super) fn dock_context_pointer(
        &mut self,
        runtime: &mut Runtime,
        target: Option<ElementId>,
        kind: usize,
        button: Option<PointerButton>,
    ) -> Result<InputResult, UiError> {
        if kind == 0
            && button == Some(PointerButton::Secondary)
            && let Some(header) = target
        {
            return self.dock_context(runtime, header, self.input.position);
        }
        Ok(Default::default())
    }
    pub(super) fn dock_context_key(
        &mut self,
        runtime: &mut Runtime,
        target: ElementId,
        event: &crate::KeyEvent,
    ) -> Result<InputResult, UiError> {
        let requested = matches!(&event.key, crate::KeyboardKey::Other(name) if name == "ContextMenu")
            && event.modifiers == Default::default()
            || matches!(&event.key, crate::KeyboardKey::Other(name) if name == "F10")
                && event.modifiers
                    == crate::Modifiers {
                        shift: true,
                        ..Default::default()
                    };
        if requested {
            let b = self.nodes[&target].bounds;
            return self.dock_context(runtime, target, [b.x, b.y + b.height]);
        }
        Ok(Default::default())
    }
    /// Active header drag in this Ui only. It neither mutates nor retains the model.
    pub fn dock_drag(&self) -> Option<DockDragInfo<'_>> {
        let drag = self
            .dock_drag
            .as_ref()
            .filter(|d| d.active && self.is_prepared())?;
        Some(DockDragInfo {
            source: drag.group,
            panel: &drag.panel,
            position: self.input.position,
            preview: drag.preview,
        })
    }
    /// Snapshots an active explicitly bound transfer source for external preview.
    /// A snapshot cannot commit a drop; take a release token to commit it.
    pub fn dock_drag_session(&self) -> Option<DockDragSession> {
        let drag = self
            .dock_drag
            .as_ref()
            .filter(|d| d.active && self.is_prepared())?;
        let source = self.dock_config(drag.root)?;
        source.surface?;
        source.transfer_scope.as_ref()?;
        source.transfer.as_ref()?;
        Some(DockDragSession {
            drag: (**drag).clone(),
            source: source.clone(),
            released: false,
        })
    }
    /// Arms an active gesture for a host-selected external destination. Call just
    /// before routing the native primary release through normal source input.
    /// Release listeners retain capture routing and may prevent the transfer.
    /// After successful routing, take the release token before the next input.
    pub fn prepare_external_dock_release(&mut self) -> bool {
        self.refresh_dock_drag();
        if self.dock_drag_session().is_none() {
            return false;
        }
        self.dock_drag.as_mut().unwrap().external_release = true;
        true
    }
    /// Takes the token produced by a normally routed, unprevented primary release
    /// after `prepare_external_dock_release`. Capture has already ended before a
    /// destination can dispatch. Request a source redraw to erase its preview.
    /// Dropping the token cancels. Cancellation, prevented release, or another
    /// source pointer/key/wheel input yields no token. This never consumes an
    /// in-progress gesture.
    pub fn take_dock_drag_session(&mut self) -> Option<DockDragSession> {
        self.dock_release.take().map(|session| *session)
    }
    /// Queries a custom host's actual destination in this prepared `Ui`.
    /// Both source snapshot and current destination policies must accept.
    /// The host owns native hit testing, overlap/occlusion and DPI conversion.
    pub fn external_dock_preview(
        &self,
        session: &DockDragSession,
        position: [f32; 2],
    ) -> Option<DockDropPreview> {
        if !self.is_prepared() || position.iter().any(|v| !v.is_finite()) {
            return None;
        }
        self.dock_destination_at(&session.drag, &session.source, None, position)
    }
    /// Consumes a release token and rechecks the destination before dispatching
    /// its transfer to the original source listener. No model changes happen
    /// automatically; applications must validate source membership on acceptance.
    /// Preview snapshots cannot commit, and tokens cannot be cloned or reused.
    pub fn drop_external_dock(
        &mut self,
        runtime: &mut Runtime,
        session: DockDragSession,
        position: [f32; 2],
    ) -> Result<InputResult, UiError> {
        if !session.released {
            return Ok(InputResult::default());
        }
        let Some(preview) = self.external_dock_preview(&session, position) else {
            return Ok(InputResult::default());
        };
        let root = self
            .pointer_target(position)
            .and_then(|hit| self.dock_location(hit))
            .unwrap()
            .0;
        let event = DockTransferEvent {
            source_surface: session.source_surface(),
            source_group: session.source_group(),
            panel: session.drag.panel.clone(),
            target_surface: self.dock_config(root).unwrap().surface.unwrap(),
            target: preview.target,
        };
        let listener = session.source.transfer.unwrap();
        if runtime.update(|cx| listener.dispatch(&event, cx))? != Dispatch::TargetGone {
            self.focus_state
                .get_or_insert_with(Default::default)
                .drop_focus = Some(Box::new((root, event.panel)));
        }
        Ok(InputResult {
            changed: true,
            default_prevented: true,
        })
    }
    pub(super) fn dock_cursor(&self) -> Option<Cursor> {
        if let Some(drag) = &self.dock_drag {
            return Some(if !drag.active {
                Cursor::Grab
            } else if drag.preview.is_some() {
                Cursor::Grabbing
            } else {
                Cursor::NotAllowed
            });
        }
        self.hovered
            .and_then(|id| self.dock_source(id))
            .map(|_| Cursor::Grab)
    }
    fn dock_destination(&self, drag: &Drag) -> Option<DockDropPreview> {
        self.dock_destination_at(
            drag,
            self.dock_config(drag.root)?,
            Some(drag.root),
            self.input.position,
        )
    }
    fn dock_target_location(
        &self,
        mut hit: ElementId,
    ) -> Option<(ElementId, ElementId, DockNodeId)> {
        loop {
            // Dock groups intentionally block pointer fallthrough. Only their
            // descendants can shield the group from becoming a drop target.
            if matches!(self.dock_metadata(hit), Some(Metadata::Group(_))) {
                return self.dock_location(hit);
            }
            let node = self.nodes.get(&hit)?;
            if node.element.pointer_events == crate::PointerEvents::Block
                || !self.input_available(hit)
                || self.is_overlay(hit)
            {
                return None;
            }
            hit = node.parent?;
        }
    }
    fn dock_destination_at(
        &self,
        drag: &Drag,
        source: &Properties,
        source_root: Option<ElementId>,
        point: [f32; 2],
    ) -> Option<DockDropPreview> {
        if source_root.is_some() && !drag.inside {
            return None;
        }
        let hit = self.pointer_target(point)?;
        let (root, group, id) = self.dock_target_location(hit)?;
        let config = self.dock_config(root)?;
        let cross = Some(root) != source_root;
        if !self.input_available(root) || !self.input_available(group) {
            return None;
        }
        if !cross && source.listener.is_none() {
            return None;
        }
        if cross
            && !(source.surface.is_some()
                && config.surface.is_some()
                && source.surface != config.surface
                && source.transfer.is_some()
                && source.transfer_scope.is_some()
                && source.transfer_scope == config.transfer_scope)
        {
            return None;
        }
        let n = &self.nodes[&group];
        let pane = n.bounds;
        let visible = pane
            .intersection(n.clip_bounds)
            .intersection(self.nodes[&root].bounds)
            .intersection(self.nodes[&root].clip_bounds);
        if !visible.contains(point) {
            return None;
        }
        let headers = *n.children.first()?;
        let list = &self.nodes[&headers];
        let entries = &list.children;
        let source_index = entries
            .iter()
            .position(|child| self.nodes[child].element.key.as_ref() == Some(&drag.panel));
        let make = |target, bounds: Bounds, insertion| {
            let query = DockDropQuery {
                source: drag.group,
                panel: drag.panel.clone(),
                source_surface: source.surface,
                target_surface: config.surface,
                target,
            };
            if source.drop_policy.as_ref().is_some_and(|p| !p(&query))
                || (cross && config.drop_policy.as_ref().is_some_and(|p| !p(&query)))
            {
                return None;
            }
            let bounds = bounds.intersection(visible);
            (bounds.width > 0. && bounds.height > 0.).then_some(DockDropPreview {
                target,
                bounds,
                insertion,
            })
        };
        if list.bounds.contains(point) && list.clip_bounds.contains(point) {
            let slot = entries
                .iter()
                .position(|child| {
                    let b = self.nodes[child].bounds;
                    point[0] < b.x + b.width * 0.5
                })
                .unwrap_or(entries.len());
            let index = slot
                - usize::from(!cross && id == drag.group && source_index.is_some_and(|i| i < slot));
            if !cross && id == drag.group && source_index == Some(index) {
                return None;
            }
            let x = entries
                .get(slot)
                .map(|child| self.nodes[child].bounds.x)
                .or_else(|| {
                    entries.last().map(|child| {
                        let b = self.nodes[child].bounds;
                        b.x + b.width
                    })
                })
                .unwrap_or(list.content_bounds.x);
            return make(
                DockDropTarget::Tab { group: id, index },
                Bounds {
                    x: x - 1.5,
                    y: list.content_bounds.y,
                    width: 3.,
                    height: list.content_bounds.height,
                }
                .intersection(list.clip_bounds),
                true,
            );
        }
        let body = self.nodes[n.children.get(1)?].bounds.intersection(visible);
        if !body.contains(point) || body.width <= 0. || body.height <= 0. {
            return None;
        }
        let distances = [
            (point[0] - body.x) / body.width,
            (body.x + body.width - point[0]) / body.width,
            (point[1] - body.y) / body.height,
            (body.y + body.height - point[1]) / body.height,
        ];
        let side = distances
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .and_then(|(index, d)| {
                (*d <= 0.25).then_some(
                    [
                        DockSide::Left,
                        DockSide::Right,
                        DockSide::Top,
                        DockSide::Bottom,
                    ][index],
                )
            });
        if let Some(side) = side {
            if !cross && id == drag.group && entries.len() < 2 {
                return None;
            }
            let horizontal = matches!(side, DockSide::Left | DockSide::Right);
            let axis = usize::from(!horizontal);
            let extent = if horizontal { pane.width } else { pane.height };
            let half = (extent - config.divider_size) * 0.5;
            if half < config.min_pane_size[axis] {
                return None;
            }
            let mut preview = pane;
            if horizontal {
                preview.width = half;
                if side == DockSide::Right {
                    preview.x = pane.x + pane.width - half;
                }
            } else {
                preview.height = half;
                if side == DockSide::Bottom {
                    preview.y = pane.y + pane.height - half;
                }
            }
            return make(
                DockDropTarget::Split {
                    group: id,
                    side,
                    position: SplitPosition::Fraction(0.5),
                },
                preview,
                false,
            );
        }
        if !cross && id == drag.group {
            return None;
        }
        make(
            DockDropTarget::Tab {
                group: id,
                index: entries.len(),
            },
            body,
            false,
        )
    }
    // A rejected drop is never a request to create a window. Require a release
    // outside every visible dock root, with no overlay or blocking hit ancestor.
    fn dock_detach_allowed(&self, point: [f32; 2]) -> bool {
        if self.order.iter().any(|id| {
            self.dock_config(*id).is_some()
                && self.nodes[id].visible
                && self.nodes[id]
                    .bounds
                    .intersection(self.nodes[id].clip_bounds)
                    .contains(point)
        }) {
            return false;
        }
        let mut current = self.pointer_target(point);
        while let Some(id) = current {
            let node = &self.nodes[&id];
            if self.is_overlay(id)
                || node.element.pointer_events == crate::PointerEvents::Block
                || !self.input_available(id)
            {
                return false;
            }
            current = node.parent;
        }
        true
    }
    pub(super) fn refresh_dock_drag(&mut self) {
        let Some(drag) = self.dock_drag.as_ref() else {
            return;
        };
        let valid = self.captured_pointer() == Some(drag.header)
            && self
                .dock_source(drag.header)
                .is_some_and(|(root, group, panel)| {
                    root == drag.root
                        && group == drag.group
                        && panel == drag.panel
                        && self.dock_config(root).is_some_and(|p| {
                            p.surface == drag.surface && p.transfer_scope == drag.scope
                        })
                });
        if !valid {
            self.cancel_capture(PointerCancelReason::TargetUnavailable);
            self.dock_drag = None;
            return;
        }
        let preview = drag.active.then(|| self.dock_destination(drag)).flatten();
        self.dock_drag.as_mut().unwrap().preview = preview;
    }
    pub(super) fn dock_pointer(
        &mut self,
        runtime: &mut Runtime,
        target: Option<ElementId>,
        kind: usize,
        button: Option<PointerButton>,
        prevented: bool,
    ) -> Result<InputResult, UiError> {
        if prevented {
            let changed = self.dock_drag.is_some();
            if changed {
                self.cancel_capture(PointerCancelReason::TargetUnavailable);
            }
            return Ok(InputResult {
                changed,
                default_prevented: false,
            });
        }
        if kind == 0
            && button == Some(PointerButton::Primary)
            && self.dock_drag.is_none()
            && self.input.capture.is_none()
            && let Some(header) = target
            && let Some((root, group, panel)) = self.dock_source(header)
        {
            let bounds = self.nodes[&header].bounds;
            let parent = self.nodes[&header]
                .parent
                .map(|id| self.nodes[&id].content_bounds);
            self.input.capture = Some(input_dispatch::Capture {
                id: header,
                button: PointerButton::Primary,
                position: self.input.position,
                bounds,
                parent,
                range: None,
                restore: None,
                axis: None,
            });
            self.dock_drag = Some(Box::new(Drag {
                root,
                header,
                group,
                panel,
                surface: self.dock_config(root).unwrap().surface,
                scope: self.dock_config(root).unwrap().transfer_scope.clone(),
                press: self.input.position,
                active: false,
                external_release: false,
                inside: true,
                preview: None,
            }));
            return Ok(InputResult {
                changed: true,
                default_prevented: false,
            });
        }
        if self.dock_drag.is_none() {
            return Ok(InputResult::default());
        }
        if kind == 2 {
            self.dock_drag.as_mut().unwrap().inside = true;
        }
        self.refresh_dock_drag();
        let Some(drag) = self.dock_drag.as_ref() else {
            return Ok(InputResult {
                changed: true,
                default_prevented: false,
            });
        };
        if kind == 1 {
            let threshold = self.dock_config(drag.root).unwrap().drag_threshold;
            let delta = [
                self.input.position[0] - drag.press[0],
                self.input.position[1] - drag.press[1],
            ];
            let active = drag.active || delta[0].hypot(delta[1]) >= threshold;
            let drag = self.dock_drag.as_mut().unwrap();
            drag.active = active;
            drag.inside = true;
            if active {
                self.pressed = None;
            }
            self.refresh_dock_drag();
            return Ok(InputResult {
                changed: active,
                default_prevented: active,
            });
        }
        if kind == 2 && button == Some(PointerButton::Primary) {
            // Hit-test again at release; motion can be coalesced by the platform.
            let preview = drag.active.then(|| self.dock_destination(drag)).flatten();
            let drag = self.dock_drag.take().unwrap();
            // Normal release ends our capture before any application callback,
            // including callbacks that return an access error.
            self.input.capture = None;
            if !drag.active {
                return Ok(InputResult {
                    changed: true,
                    default_prevented: false,
                });
            }
            self.pressed = None;
            if drag.external_release {
                let source = self.dock_config(drag.root).unwrap().clone();
                self.dock_release = Some(Box::new(DockDragSession {
                    drag: *drag,
                    source,
                    released: true,
                }));
                return Ok(InputResult {
                    changed: true,
                    default_prevented: true,
                });
            }
            if let Some(preview) = preview {
                let destination = self
                    .pointer_target(self.input.position)
                    .and_then(|hit| self.dock_location(hit))
                    .map(|(root, _, _)| root)
                    .unwrap();
                let completion = (destination, drag.panel.clone());
                let config = self.dock_config(drag.root).unwrap();
                let dispatched = if destination != drag.root {
                    let listener = config.transfer.clone().unwrap();
                    let event = DockTransferEvent {
                        source_surface: config.surface.unwrap(),
                        source_group: drag.group,
                        panel: drag.panel,
                        target_surface: self.dock_config(destination).unwrap().surface.unwrap(),
                        target: preview.target,
                    };
                    runtime.update(|cx| listener.dispatch(&event, cx))?
                } else if let Some(listener) = config.listener.clone() {
                    let event = DockEvent::Drop {
                        source: drag.group,
                        panel: drag.panel,
                        target: preview.target,
                    };
                    runtime.update(|cx| listener.dispatch(&event, cx))?
                } else {
                    Dispatch::TargetGone
                };
                if dispatched != Dispatch::TargetGone {
                    self.focus_state
                        .get_or_insert_with(Default::default)
                        .drop_focus = Some(Box::new(completion));
                }
            } else if self.dock_detach_allowed(self.input.position)
                && let Some(config) = self.dock_config(drag.root)
                && let (Some(surface), Some(listener)) = (config.surface, config.detach.clone())
            {
                let event = DockDetachEvent {
                    source_surface: surface,
                    source_group: drag.group,
                    panel: drag.panel,
                    position: self.input.position,
                };
                runtime.update(|cx| listener.dispatch(&event, cx))?;
            }
            return Ok(InputResult {
                changed: true,
                default_prevented: true,
            });
        }
        Ok(InputResult {
            changed: false,
            default_prevented: drag.active,
        })
    }
    #[cfg(feature = "rendering")]
    pub(crate) fn dock_preview_paint(&self) -> Option<(DockDropPreview, Bounds, Color)> {
        let preview = self.dock_drag()?.preview?;
        if !self
            .dock_config(self.dock_drag.as_ref()?.root)?
            .show_preview
        {
            return None;
        }
        let root = self
            .pointer_target(self.input.position)
            .and_then(|hit| self.dock_location(hit))?
            .0;
        if !self.dock_config(root)?.show_preview {
            return None;
        }
        let n = &self.nodes[&root];
        let mut color = n.resolved_theme.color(ThemeColor::Focus);
        let mut current = Some(root);
        while let Some(id) = current {
            let n = &self.nodes[&id];
            color[3] *= n.element.opacity;
            current = n.parent;
        }
        Some((preview, n.bounds.intersection(n.clip_bounds), color))
    }
    pub(super) fn restore_dock_drop_focus(&mut self) {
        let Some(pending) = self.focus_state.as_mut().and_then(|s| s.drop_focus.take()) else {
            return;
        };
        let (root, panel) = *pending;
        let header = self.order.iter().copied().find(|id| {
            let is_panel = matches!(
                self.tab_properties(*id),
                Some(crate::tabs::Properties::Header { key, .. }) if key == &panel
            );
            if !is_panel || !self.enabled(*id) {
                return false;
            }
            self.dock_location(*id).is_some_and(|(current, group, _)| {
                current == root
                    && matches!(
                        self.tab_properties(group),
                        Some(crate::tabs::Properties::Root { selected, .. })
                            if selected.as_ref() == Some(&panel)
                    )
            })
        });
        if let Some(header) = header {
            self.change_focus(Some(header));
            self.reveal(header);
        }
    }
}
