//! Placement-local header drag defaults and geometric docking previews.
use super::*;
use crate::{
    DockDropTarget, DockEvent, DockNodeId, DockSide, SplitPosition,
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
pub(super) struct Drag {
    root: ElementId,
    header: ElementId,
    group: DockNodeId,
    panel: Key,
    press: [f32; 2],
    active: bool,
    pub(super) inside: bool,
    pub(super) preview: Option<DockDropPreview>,
}
impl<T: View> Ui<T> {
    fn dock_metadata(&self, id: ElementId) -> Option<&Metadata> {
        self.nodes.get(&id)?.element.input.as_ref()?.dock.as_deref()
    }
    fn dock_location(&self, mut id: ElementId) -> Option<(ElementId, ElementId, DockNodeId)> {
        let mut group = None;
        loop {
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
    fn dock_source(&self, header: ElementId) -> Option<(ElementId, DockNodeId, Key)> {
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
        let config = self.dock_config(root)?;
        (config.draggable
            && config.listener.is_some()
            && self.input_available(header)
            && self.input_available(root))
        .then(|| (root, group, key.clone()))
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
        let point = self.input.position;
        if !drag.inside {
            return None;
        }
        let hit = self.pointer_target(point)?;
        let (root, group, id) = self.dock_location(hit)?;
        if root != drag.root || !self.input_available(group) {
            return None;
        }
        let config = self.dock_config(root)?;
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
            let index =
                slot - usize::from(id == drag.group && source_index.is_some_and(|i| i < slot));
            if id == drag.group && source_index == Some(index) {
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
            if id == drag.group && entries.len() < 2 {
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
        if id == drag.group {
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
    pub(super) fn refresh_dock_drag(&mut self) {
        let Some(drag) = self.dock_drag.as_ref() else {
            return;
        };
        let valid = self.captured_pointer() == Some(drag.header)
            && self
                .dock_source(drag.header)
                .is_some_and(|(root, group, panel)| {
                    root == drag.root && group == drag.group && panel == drag.panel
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
                press: self.input.position,
                active: false,
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
            if let Some(preview) = preview
                && let Some(listener) = self.dock_config(drag.root).and_then(|p| p.listener.clone())
            {
                let completion = (drag.root, drag.panel.clone());
                let event = DockEvent::Drop {
                    source: drag.group,
                    panel: drag.panel,
                    target: preview.target,
                };
                if runtime.update(|cx| listener.dispatch(&event, cx))? == Dispatch::Handled {
                    self.focus_state
                        .get_or_insert_with(Default::default)
                        .drop_focus = Some(Box::new(completion));
                }
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
        let root = self.dock_drag.as_ref()?.root;
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
