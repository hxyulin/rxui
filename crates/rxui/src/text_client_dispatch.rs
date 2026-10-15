//! Committed text and IME composition for application-defined text input clients.
use super::*;
use crate::input::{
    EventPhase, ImeCancelReason, ImeEvent, ImeInput, Requests, TextClient, TextInsertInput,
    TextInsertSource,
};
use std::cell::Cell;

type ImeRoute = Vec<(ElementId, crate::Listener<ImeInput>)>;

/// Active composition of one client.
pub(super) struct Composition {
    pub id: ElementId,
    /// IME route captured at the last preedit, so a removed client's surviving
    /// owner and ancestor listeners still receive the cancellation.
    route: ImeRoute,
}
#[derive(Default)]
pub(super) struct State {
    pub composition: Option<Composition>,
    /// Changes whenever the framework cancels a client's composition, so the host
    /// resets the platform IME session.
    pub revision: u64,
    /// Cancellations queued without runtime access: target, route and reason.
    pending: Vec<(ElementId, ImeRoute, ImeCancelReason)>,
}
impl State {
    pub(super) fn pending(&self) -> bool {
        !self.pending.is_empty()
    }
}
impl<T: View> Ui<T> {
    fn text_client_properties(&self, id: ElementId) -> Option<&TextClient> {
        let node = self.nodes.get(&id)?;
        if node.editor.is_some() {
            return None;
        }
        node.element
            .input
            .as_ref()?
            .text
            .as_deref()
            .filter(|t| t.enabled)
    }
    /// Whether `id` is a live, enabled text input client, regardless of activation.
    pub(super) fn is_text_client(&self, id: ElementId) -> bool {
        self.text_client_properties(id).is_some() && self.enabled(id)
    }
    /// The focused element when it is a text input client accepting text: marked
    /// with `Element::text_input_client(true)`, enabled, and in an active placement.
    /// Hosts route committed text and IME events to it through [`Self::text_input`].
    pub fn focused_text_client(&self) -> Option<ElementId> {
        self.focused
            .filter(|id| self.active && self.is_text_client(*id))
    }
    /// Text client listeners from `target` to the root, in bubble order.
    fn text_client_route<L>(
        &self,
        target: ElementId,
        available: bool,
        listener: impl Fn(&TextClient) -> Option<&crate::Listener<L>>,
    ) -> Vec<(ElementId, crate::Listener<L>)> {
        let mut route = Vec::new();
        let mut current = Some(target);
        while let Some(id) = current {
            let Some(node) = self.nodes.get(&id) else {
                break;
            };
            if (!available || self.input_available(id))
                && let Some(l) = node
                    .element
                    .input
                    .as_ref()
                    .and_then(|p| p.text.as_deref())
                    .and_then(&listener)
            {
                route.push((id, l.clone()));
            }
            current = node.parent;
        }
        route
    }
    fn dispatch_ime(
        runtime: &mut Runtime,
        target: ElementId,
        route: &ImeRoute,
        event: &ImeEvent,
    ) -> Result<bool, UiError> {
        let mut changed = false;
        for (id, listener) in route {
            let input = ImeInput {
                target,
                current_target: *id,
                phase: if *id == target {
                    EventPhase::Target
                } else {
                    EventPhase::Bubble
                },
                event: event.clone(),
                requests: Cell::new(Requests::default()),
            };
            changed |= runtime.update(|cx| listener.dispatch(&input, cx))? == Dispatch::Handled;
            if input.requests.get().stop {
                break;
            }
        }
        Ok(changed)
    }
    fn dispatch_text_insert(
        &self,
        runtime: &mut Runtime,
        target: ElementId,
        text: String,
        source: TextInsertSource,
    ) -> Result<bool, UiError> {
        let route = self.text_client_route(target, true, |t| t.insert.as_ref());
        let mut changed = false;
        for (id, listener) in &route {
            let input = TextInsertInput {
                target,
                current_target: *id,
                phase: if *id == target {
                    EventPhase::Target
                } else {
                    EventPhase::Bubble
                },
                text: text.clone(),
                source,
                requests: Cell::new(Requests::default()),
            };
            changed |= runtime.update(|cx| listener.dispatch(&input, cx))? == Dispatch::Handled;
            if input.requests.get().stop {
                break;
            }
        }
        Ok(changed)
    }
    /// Ends the active client composition, queueing its cancellation for the next
    /// flush and resetting the host's IME session. Returns whether one was active.
    pub(super) fn cancel_client_composition(&mut self, reason: ImeCancelReason) -> bool {
        let Some(composition) = self.text_client.composition.take() else {
            return false;
        };
        let id = composition.id;
        let route = if self.nodes.contains_key(&id) {
            self.text_client_route(id, false, |t| t.ime.as_ref())
        } else {
            composition.route
        };
        self.text_client.pending.push((id, route, reason));
        self.text_client.revision = self.text_client.revision.wrapping_add(1);
        true
    }
    /// Cancels the client composition when its client was removed, stopped taking
    /// text or lost focus. Runs after focus changes and reconciliation.
    pub(super) fn validate_client_composition(&mut self) {
        let Some(id) = self.text_client.composition.as_ref().map(|c| c.id) else {
            return;
        };
        let reason = if !self.nodes.contains_key(&id) {
            ImeCancelReason::Removed
        } else if !self.is_text_client(id) {
            ImeCancelReason::Unavailable
        } else if self.focused != Some(id) {
            ImeCancelReason::FocusChanged
        } else {
            return;
        };
        self.cancel_client_composition(reason);
    }
    /// Cancels composition before a primary press reaches the composing client or
    /// one of its descendants, so the client can move its caret afterwards.
    pub(super) fn cancel_client_composition_for_press(
        &mut self,
        runtime: &mut Runtime,
        target: Option<ElementId>,
    ) -> Result<bool, UiError> {
        let Some(id) = self.text_client.composition.as_ref().map(|c| c.id) else {
            return Ok(false);
        };
        let mut current = target;
        while let Some(t) = current {
            if t == id {
                self.cancel_client_composition(ImeCancelReason::Pointer);
                self.flush_text_client(runtime)?;
                return Ok(true);
            }
            current = self.nodes.get(&t).and_then(|n| n.parent);
        }
        Ok(false)
    }
    /// Dispatches queued client cancellations in order.
    pub(super) fn flush_text_client(&mut self, runtime: &mut Runtime) -> Result<bool, UiError> {
        let mut changed = false;
        for (target, route, reason) in std::mem::take(&mut self.text_client.pending) {
            changed |= Self::dispatch_ime(runtime, target, &route, &ImeEvent::Cancel(reason))?;
        }
        Ok(changed)
    }
    /// Revision for [`Self::ime_reset_revision`] while a client is focused.
    pub(super) fn client_ime_revision(&self) -> u64 {
        self.text_client.revision
    }
    /// The focused client's IME candidate area in window logical units.
    pub(super) fn client_ime_area(&self, id: ElementId) -> Result<Option<Bounds>, UiError> {
        if !self.is_prepared() {
            return Ok(None);
        }
        let node = &self.nodes[&id];
        if !node.visible {
            return Ok(None);
        }
        let Some(area) = self
            .text_client_properties(id)
            .and_then(|t| t.ime_area_provider.as_ref().map_or(t.ime_area, |get| get()))
        else {
            return Ok(Some(node.content_bounds));
        };
        if [area.x, area.y, area.width, area.height]
            .iter()
            .any(|v| !v.is_finite())
            || area.width < 0.
            || area.height < 0.
        {
            return Err(UiError::InvalidGeometry);
        }
        let area = Bounds {
            x: node.bounds.x + area.x,
            y: node.bounds.y + area.y,
            ..area
        };
        // Translation can overflow even when the local rectangle is finite.
        if !area.x.is_finite() || !area.y.is_finite() {
            return Err(UiError::InvalidGeometry);
        }
        Ok(Some(area))
    }
    /// Routes one host text event to the focused client `id`; see `Ui::text_input`.
    pub(super) fn client_text_input(
        &mut self,
        runtime: &mut Runtime,
        id: ElementId,
        event: TextInputEvent,
    ) -> Result<bool, UiError> {
        // Cancellations queued by earlier focus changes arrive before new input.
        let mut changed = self.flush_text_client(runtime)?;
        let composing = self
            .text_client
            .composition
            .as_ref()
            .is_some_and(|c| c.id == id);
        match event {
            // Keyboard text and paste wait for the composition to end, as in
            // built-in inputs.
            TextInputEvent::Insert(text) if !composing && !text.is_empty() => {
                changed |=
                    self.dispatch_text_insert(runtime, id, text, TextInsertSource::Keyboard)?
            }
            TextInputEvent::Paste(text) if !composing && !text.is_empty() => {
                changed |= self.dispatch_text_insert(runtime, id, text, TextInsertSource::Paste)?
            }
            TextInputEvent::Commit(text) if composing || !text.is_empty() => {
                // A commit ends composition without resetting the IME session.
                self.text_client.composition = None;
                changed |= composing;
                changed |= self.dispatch_text_insert(runtime, id, text, TextInsertSource::Ime)?;
            }
            TextInputEvent::Preedit { text, cursor } => {
                if text.chars().any(char::is_control)
                    || cursor.is_some_and(|(a, b)| {
                        a > b
                            || b > text.len()
                            || !text.is_char_boundary(a)
                            || !text.is_char_boundary(b)
                    })
                {
                    return Err(UiError::InvalidTextValue);
                }
                // Platforms send empty preedits around commits; outside a
                // composition they carry nothing to display.
                if text.is_empty() && !composing {
                    return Ok(changed);
                }
                let route = self.text_client_route(id, true, |t| t.ime.as_ref());
                let event = ImeEvent::Preedit { text, cursor };
                Self::dispatch_ime(runtime, id, &route, &event)?;
                self.text_client.composition = Some(Composition { id, route });
                // The host needs the preedit and candidate area either way.
                changed = true;
            }
            TextInputEvent::CancelComposition => {
                changed |= self.cancel_client_composition(ImeCancelReason::Host);
                changed |= self.flush_text_client(runtime)?;
            }
            // Clients handle editing keys, Enter, history and selection through
            // their key listeners and commands.
            _ => {}
        }
        Ok(changed)
    }
}
