use super::*;
use crate::input::{
    Cursor, EventPhase, InputResult, KeyEvent, KeyInput, KeyboardKey, Modifiers, PointerButton,
    PointerButtons, PointerCancelReason, PointerInput, Requests,
};
use std::cell::Cell;

#[derive(Clone, Copy)]
pub(super) struct Capture {
    pub id: ElementId,
    pub button: PointerButton,
    pub position: [f32; 2],
    pub bounds: Bounds,
    pub parent: Option<Bounds>,
    pub range: Option<[f32; 2]>,
    pub restore: Option<crate::SplitPosition>,
    pub axis: Option<crate::Axis>,
}
#[derive(Default)]
pub(super) struct State {
    pub capture: Option<Capture>,
    pub position: [f32; 2],
    pub buttons: PointerButtons,
    pub modifiers: Modifiers,
    pub prevented: bool,
    route: Vec<ElementId>,
    pending: Vec<(crate::Listener<PointerInput>, PointerInput)>,
    pending_scroll: Option<(ElementId, [f32; 2])>,
    pending_resize: Option<(crate::Listener<crate::ResizeEvent>, crate::ResizeEvent)>,
}
impl State {
    pub(super) fn pending(&self) -> bool {
        !self.pending.is_empty() || self.pending_scroll.is_some() || self.pending_resize.is_some()
    }
}
impl PointerEvent {
    pub(super) fn parts(self) -> (usize, Option<[f32; 2]>, Option<PointerButton>, Modifiers) {
        match self {
            Self::Moved(p) => (1, Some(p), None, Modifiers::default()),
            Self::Pressed(p) => (
                0,
                Some(p),
                Some(PointerButton::Primary),
                Modifiers::default(),
            ),
            Self::Released(p) => (
                2,
                Some(p),
                Some(PointerButton::Primary),
                Modifiers::default(),
            ),
            Self::Motion {
                position,
                modifiers,
            } => (1, Some(position), None, modifiers),
            Self::Down {
                position,
                button,
                modifiers,
            } => (0, Some(position), Some(button), modifiers),
            Self::Up {
                position,
                button,
                modifiers,
            } => (2, Some(position), Some(button), modifiers),
            Self::Cancelled => (3, None, None, Modifiers::default()),
            Self::Left => (4, None, None, Modifiers::default()),
        }
    }
}
impl<T: View> Ui<T> {
    pub(super) fn input_available(&self, id: ElementId) -> bool {
        if !self.modal_allows(id) {
            return false;
        }
        self.input_available_unconfined(id)
    }
    pub(super) fn input_available_unconfined(&self, id: ElementId) -> bool {
        let Some(node) = self.nodes.get(&id) else {
            return false;
        };
        if !node.visible
            || node.inert
            || self.range_unavailable(id)
            || matches!(
                node.element.kind,
                ElementKind::Button { disabled: true, .. }
                    | ElementKind::TextInput { disabled: true, .. }
            )
        {
            return false;
        }
        let mut current = Some(id);
        while let Some(id) = current {
            let n = &self.nodes[&id];
            if n.element.style.display == Display::None {
                return false;
            }
            current = n.parent;
        }
        true
    }
    fn input_parent_bounds(&self, id: ElementId) -> Option<Bounds> {
        self.nodes
            .get(&id)
            .and_then(|n| n.parent)
            .and_then(|p| self.nodes.get(&p))
            .map(|n| n.content_bounds)
    }
    fn input_payload(
        &self,
        target: ElementId,
        current: ElementId,
        hit: Option<ElementId>,
        phase: EventPhase,
        button: Option<PointerButton>,
        cancel: Option<PointerCancelReason>,
    ) -> PointerInput {
        let bounds = self.nodes[&current].bounds;
        let capture = self.input.capture;
        PointerInput {
            target,
            current_target: current,
            hit_target: hit,
            phase,
            position: self.input.position,
            local_position: [
                self.input.position[0] - bounds.x,
                self.input.position[1] - bounds.y,
            ],
            bounds,
            parent_bounds: self.input_parent_bounds(current),
            button,
            buttons: self.input.buttons,
            modifiers: self.input.modifiers,
            press_position: capture.map(|c| c.position),
            press_bounds: capture.map(|c| c.bounds),
            press_parent_bounds: capture.and_then(|c| c.parent),
            cancel_reason: cancel,
            requests: Cell::new(Requests::default()),
        }
    }
    fn input_path(&self, target: ElementId, path: &mut Vec<ElementId>) {
        path.clear();
        let mut current = Some(target);
        while let Some(id) = current {
            path.push(id);
            current = self.nodes.get(&id).and_then(|n| n.parent);
        }
    }
    pub(super) fn cancel_capture(&mut self, reason: PointerCancelReason) {
        self.dock_drag = None;
        let Some(capture) = self.input.capture else {
            return;
        };
        let mut path = std::mem::take(&mut self.input.route);
        self.input_path(capture.id, &mut path);
        for (current, capturing) in path
            .iter()
            .rev()
            .map(|id| (*id, true))
            .chain(path.iter().map(|id| (*id, false)))
        {
            if let Some(listener) = self
                .nodes
                .get(&current)
                .and_then(|n| n.element.input.as_ref())
                .and_then(|p| p.pointer[3 + 4 * usize::from(capturing)].clone())
            {
                let event = self.input_payload(
                    capture.id,
                    current,
                    None,
                    if current == capture.id {
                        EventPhase::Target
                    } else if capturing {
                        EventPhase::Capture
                    } else {
                        EventPhase::Bubble
                    },
                    None,
                    Some(reason),
                );
                self.input.pending.push((listener, event));
            }
        }
        if let Some(values) = capture.range {
            if let Some((listener, mut event)) =
                self.resize_event(capture.id, values[0], crate::ResizePhase::Cancel)
            {
                if reason != PointerCancelReason::Escape {
                    event.first_size = self.range_info(capture.id).map_or(values[0], |r| r.value);
                    event.position =
                        if let ElementKind::Splitter(p) = &self.nodes[&capture.id].element.kind {
                            p.position
                        } else {
                            event.position
                        };
                } else if let Some(restore) = capture.restore {
                    event.position = restore;
                    let desired = match restore {
                        crate::SplitPosition::Fraction(v) => v * event.available,
                        crate::SplitPosition::Pixels(v) => v,
                    };
                    if let Some(info) = self.range_info(capture.id) {
                        event.first_size = desired.clamp(info.min, info.max);
                    }
                }
                self.input.pending_resize = Some((listener, event));
            } else if reason == PointerCancelReason::Escape
                && let Some(info) = self.range_info(capture.id)
                && let ElementKind::Scrollbar(p) = &self.nodes[&capture.id].element.kind
                && let Some(target) = p.handle.element_in(self.tree)
            {
                let mut offset = self.nodes[&target].scroll_offset;
                offset[info.axis.index()] = values[0];
                self.input.pending_scroll = Some((target, offset));
            }
        }
        self.input.capture = None;
        self.input.route = path;
        self.pressed = None;
    }
    pub(super) fn flush_input_cancellations(
        &mut self,
        runtime: &mut Runtime,
    ) -> Result<bool, UiError> {
        let pending = std::mem::take(&mut self.input.pending);
        let mut changed = false;
        let mut prevented = false;
        for (listener, event) in pending {
            changed |= runtime.update(|cx| listener.dispatch(&event, cx))? == Dispatch::Handled;
            prevented |= event.requests.get().prevent;
            if event.requests.get().stop {
                break;
            }
        }
        let resize = self.input.pending_resize.take();
        let scroll = self.input.pending_scroll.take();
        if !prevented {
            if let Some((listener, event)) = resize {
                changed |= runtime.update(|cx| listener.dispatch(&event, cx))? == Dispatch::Handled;
            }
            if let Some((target, offset)) = scroll {
                changed |= self.set_scroll_offset(target, offset)?;
            }
        }
        Ok(changed)
    }
    pub(super) fn cancel_invalid_capture(&mut self) {
        if self.input.capture.is_some_and(|c| {
            !self.input_available(c.id)
                || !self.pointer_allowed(c.id)
                || self
                    .range_info(c.id)
                    .is_some_and(|r| r.read_only || c.axis.is_some_and(|a| a != r.axis))
        }) {
            self.cancel_capture(PointerCancelReason::TargetUnavailable);
        }
    }
    /// Explicit mouse capture owner, separate from built-in control press capture.
    pub fn captured_pointer(&self) -> Option<ElementId> {
        self.input.capture.map(|c| c.id)
    }
    /// Cursor from the capture owner or current pointer target, then its ancestors.
    pub fn cursor(&self) -> Cursor {
        if let Some(cursor) = self.dock_cursor() {
            return cursor;
        }
        let mut current = self.captured_pointer().or(self.hovered);
        while let Some(id) = current {
            let Some(n) = self.nodes.get(&id) else {
                break;
            };
            if let Some(cursor) = n.element.input.as_ref().and_then(|p| p.cursor) {
                return cursor;
            }
            if n.editor.is_some() {
                return Cursor::Text;
            }
            match &n.element.kind {
                ElementKind::Scrollbar(_) => return Cursor::Default,
                ElementKind::Splitter(p) => {
                    return if p.axis == crate::Axis::Horizontal {
                        Cursor::ResizeHorizontal
                    } else {
                        Cursor::ResizeVertical
                    };
                }
                _ => {}
            }
            current = n.parent;
        }
        Cursor::Default
    }
    pub(super) fn dispatch_pointer(
        &mut self,
        runtime: &mut Runtime,
        target: ElementId,
        hit: Option<ElementId>,
        kind: usize,
        button: Option<PointerButton>,
    ) -> Result<InputResult, UiError> {
        let mut path = std::mem::take(&mut self.input.route);
        self.input_path(target, &mut path);
        let result = (|| {
            let mut result = InputResult::default();
            for (id, capturing) in path
                .iter()
                .rev()
                .map(|id| (*id, true))
                .chain(path.iter().map(|id| (*id, false)))
            {
                if !self.input_available(id) {
                    continue;
                }
                let Some(listener) = self.nodes[&id]
                    .element
                    .input
                    .as_ref()
                    .and_then(|p| p.pointer[kind + 4 * usize::from(capturing)].clone())
                else {
                    continue;
                };
                let event = self.input_payload(
                    target,
                    id,
                    hit,
                    if id == target {
                        EventPhase::Target
                    } else if capturing {
                        EventPhase::Capture
                    } else {
                        EventPhase::Bubble
                    },
                    button,
                    None,
                );
                result.changed |=
                    runtime.update(|cx| listener.dispatch(&event, cx))? == Dispatch::Handled;
                let requests = event.requests.get();
                result.default_prevented |= requests.prevent;
                if requests.focus && self.enabled(id) {
                    self.change_focus(Some(id));
                    result.changed = true;
                }
                match requests.capture {
                    Some(true) if self.input.buttons.any() => {
                        if self.captured_pointer() != Some(id) {
                            self.cancel_capture(PointerCancelReason::TargetUnavailable);
                            self.input.capture = Some(Capture {
                                id,
                                button: button.unwrap_or_else(|| {
                                    [
                                        PointerButton::Primary,
                                        PointerButton::Secondary,
                                        PointerButton::Middle,
                                    ]
                                    .into_iter()
                                    .find(|b| self.input.buttons.contains(*b))
                                    .expect("held button")
                                }),
                                position: self.input.position,
                                bounds: self.nodes[&id].bounds,
                                parent: self.input_parent_bounds(id),
                                range: None,
                                restore: None,
                                axis: None,
                            });
                        }
                    }
                    Some(false) if self.captured_pointer() == Some(id) => self.input.capture = None,
                    _ => {}
                }
                if requests.stop {
                    break;
                }
            }
            Ok(result)
        })();
        self.input.route = path;
        result
    }
    /// General mouse routing plus existing control defaults. Coordinates are logical;
    /// capture is local to this UI placement. Custom hosts forward cancellation on
    /// deactivation/device cancellation and keep geometry prepared before input.
    pub(super) fn pointer_general(
        &mut self,
        runtime: &mut Runtime,
        event: PointerEvent,
    ) -> Result<bool, UiError> {
        runtime.is_dirty(&self.owner)?;
        self.input.prevented = false;
        let (kind, position, button, modifiers) = event.parts();
        if position.is_some_and(|p| p.iter().any(|v| !v.is_finite())) {
            return Err(UiError::InvalidGeometry);
        }
        let before = (
            self.hovered,
            self.pressed,
            self.focused,
            self.captured_pointer(),
            self.cursor(),
        );
        if kind == 3 {
            self.cancel_capture(PointerCancelReason::Host);
            self.input.buttons = PointerButtons::default();
            self.hovered = None;
            self.pressed = None;
            let changed = self.flush_input_cancellations(runtime)?;
            return Ok(changed || before.0.is_some() || before.1.is_some() || before.3.is_some());
        }
        if !self.geometry_ready || !self.active {
            return Ok(false);
        }
        if kind == 4 {
            let cleared = self.dock_drag.as_mut().is_some_and(|drag| {
                drag.inside = false;
                drag.preview.take().is_some()
            });
            self.hovered = None;
            return Ok(cleared || before.0.is_some());
        }
        self.input.position = position.unwrap();
        self.input.modifiers = modifiers;
        if let Some(button) = button {
            self.input.buttons.set(button, kind == 0);
        }
        let hit = self.pointer_target(self.input.position);
        self.hovered = hit.filter(|id| self.input_available(*id));
        let target = self
            .captured_pointer()
            .or({
                if kind == 1 || kind == 2 {
                    self.pressed
                } else {
                    None
                }
            })
            .or(hit);
        let mut result = if let Some(target) = target {
            self.dispatch_pointer(runtime, target, hit, kind, button)?
        } else {
            InputResult::default()
        };
        self.input.prevented = result.default_prevented;
        let overlay = self.overlay_pointer(runtime, kind, result.default_prevented)?;
        result.changed |= overlay.changed;
        result.default_prevented |= overlay.default_prevented;
        if !result.default_prevented {
            let context = self.dock_context_pointer(runtime, target, kind, button)?;
            result.changed |= context.changed;
            result.default_prevented |= context.default_prevented;
        }
        let dock = self.dock_pointer(runtime, target, kind, button, result.default_prevented)?;
        result.changed |= dock.changed;
        result.default_prevented |= dock.default_prevented;
        self.input.prevented = result.default_prevented;
        if !result.default_prevented
            && let Some(target) = target
            && let Some(changed) = self.range_pointer(runtime, target, kind, button)?
        {
            result.changed |= changed;
            result.default_prevented = true;
            self.input.prevented = true;
        }
        if !result.default_prevented {
            if kind == 0 && button == Some(PointerButton::Primary) {
                self.change_focus(target.filter(|id| self.enabled(*id)));
                self.pressed = target.filter(|id| {
                    self.input_available(*id)
                        && matches!(
                            self.nodes[id].element.kind,
                            ElementKind::Button { .. } | ElementKind::TextInput { .. }
                        )
                });
            } else if kind == 2 && button == Some(PointerButton::Primary) {
                let pressed = self.pressed.take();
                if let Some(id) = pressed
                    && hit == Some(id)
                    && target == Some(id)
                {
                    result.changed |= self.activate(runtime, id)?;
                }
            }
        } else if kind == 2 && button == Some(PointerButton::Primary) {
            self.pressed = None;
        }
        if kind == 2 && self.input.capture.is_some_and(|c| button == Some(c.button)) {
            self.input.capture = None;
        }
        result.changed |= self.flush_input_cancellations(runtime)?;
        Ok(result.changed
            || before
                != (
                    self.hovered,
                    self.pressed,
                    self.focused,
                    self.captured_pointer(),
                    self.cursor(),
                ))
    }
    /// Routes keyboard listeners through focus/root. Custom hosts must honor
    /// default_prevented before their own text/control defaults. Escape cancels
    /// explicit capture unless prevented. Key-up has no control default here.
    pub fn key(&mut self, runtime: &mut Runtime, event: KeyEvent) -> Result<InputResult, UiError> {
        runtime.is_dirty(&self.owner)?;
        if !self.is_prepared() || !self.active {
            return Ok(InputResult::default());
        }
        let Some(target) = self.focused.or(self.root) else {
            return Ok(InputResult::default());
        };
        let mut path = std::mem::take(&mut self.input.route);
        self.input_path(target, &mut path);
        let result = (|| {
            let mut result = InputResult::default();
            for (id, capturing) in path
                .iter()
                .rev()
                .map(|id| (*id, true))
                .chain(path.iter().map(|id| (*id, false)))
            {
                if !self.input_available(id) {
                    continue;
                }
                let Some(listener) = self.nodes[&id].element.input.as_ref().and_then(|p| {
                    p.key[usize::from(!event.pressed) + 2 * usize::from(capturing)].clone()
                }) else {
                    continue;
                };
                let input = KeyInput {
                    target,
                    current_target: id,
                    phase: if id == target {
                        EventPhase::Target
                    } else if capturing {
                        EventPhase::Capture
                    } else {
                        EventPhase::Bubble
                    },
                    event: event.clone(),
                    requests: Cell::new(Requests::default()),
                };
                result.changed |=
                    runtime.update(|cx| listener.dispatch(&input, cx))? == Dispatch::Handled;
                let requests = input.requests.get();
                result.default_prevented |= requests.prevent;
                if requests.stop {
                    break;
                }
            }
            if event.pressed && !result.default_prevented {
                let command = self.command_key(runtime, &event)?;
                result.changed |= command.changed;
                result.default_prevented |= command.default_prevented;
            }
            if event.pressed && !result.default_prevented {
                let context = self.dock_context_key(runtime, target, &event)?;
                result.changed |= context.changed;
                result.default_prevented |= context.default_prevented;
            }
            if event.pressed && !result.default_prevented {
                let overlay = self.overlay_key(runtime, &event)?;
                result.changed |= overlay.changed;
                result.default_prevented |= overlay.default_prevented;
            }
            if event.pressed
                && !result.default_prevented
                && let Some(changed) = self.tab_key(runtime, target, &event)?
            {
                result.changed |= changed;
                result.default_prevented = true;
            }
            if event.pressed
                && !result.default_prevented
                && let Some(changed) = self.range_key(runtime, target, &event.key)?
            {
                result.changed |= changed;
                result.default_prevented = true;
            }
            if event.pressed
                && event.key == KeyboardKey::Escape
                && !result.default_prevented
                && self.captured_pointer().is_some()
            {
                self.cancel_capture(PointerCancelReason::Escape);
                result.changed = true;
                result.changed |= self.flush_input_cancellations(runtime)?;
            }
            Ok(result)
        })();
        self.input.route = path;
        result
    }
}
