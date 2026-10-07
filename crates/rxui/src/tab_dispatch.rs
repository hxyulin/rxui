use super::*;
use crate::{TabActivation, TabSelectEvent};

impl<T: View> Ui<T> {
    pub(super) fn tab_properties(&self, id: ElementId) -> Option<&crate::tabs::Properties> {
        self.nodes.get(&id)?.element.input.as_ref()?.tabs.as_deref()
    }
    fn tab_root(&self, mut id: ElementId) -> Option<ElementId> {
        loop {
            if matches!(
                self.tab_properties(id),
                Some(crate::tabs::Properties::Root { .. })
            ) {
                return Some(id);
            }
            id = self.nodes.get(&id)?.parent?;
        }
    }
    pub(super) fn tab_relations(&self, id: ElementId) -> (Option<ElementId>, Option<ElementId>) {
        let (key, panel) = match self.tab_properties(id) {
            Some(crate::tabs::Properties::Header { key, .. }) => (key, false),
            Some(crate::tabs::Properties::Panel { key }) => (key, true),
            _ => return (None, None),
        };
        let Some(root) = self.tab_root(id) else {
            return (None, None);
        };
        let other = self.focus_state.as_ref().and_then(|s| {
            s.nodes.iter().copied().find(|other| {
                self.tab_root(*other) == Some(root)
                    && self.semantic_visible(*other)
                    && match self.tab_properties(*other) {
                        Some(crate::tabs::Properties::Panel { key: other }) if !panel => {
                            key == other
                        }
                        Some(crate::tabs::Properties::Header { key: other, .. }) if panel => {
                            key == other
                        }
                        _ => false,
                    }
            })
        });
        if panel { (other, None) } else { (None, other) }
    }
    pub(super) fn tab_key(
        &mut self,
        runtime: &mut Runtime,
        target: ElementId,
        event: &crate::KeyEvent,
    ) -> Result<Option<bool>, UiError> {
        use crate::{Axis, KeyboardKey};
        if event.modifiers.control || event.modifiers.alt || event.modifiers.meta {
            return Ok(None);
        }
        let Some(crate::tabs::Properties::Header {
            close, close_event, ..
        }) = self.tab_properties(target)
        else {
            return Ok(None);
        };
        if event.key == KeyboardKey::Delete {
            let close = close.clone();
            let close_event = close_event.clone();
            return match close {
                Some(listener) => Ok(Some(
                    runtime.update(|cx| listener.dispatch(&close_event, cx))? == Dispatch::Handled,
                )),
                None => Ok(None),
            };
        }
        let mut parent = self.nodes[&target].parent;
        let mut list = None;
        while let Some(id) = parent {
            if let Some(crate::tabs::Properties::List { axis, activation }) =
                self.tab_properties(id)
            {
                list = Some((id, *axis, *activation));
                break;
            }
            parent = self.nodes[&id].parent;
        }
        let Some((list, axis, activation)) = list else {
            return Ok(None);
        };
        let direction = match event.key {
            KeyboardKey::Home => 0,
            KeyboardKey::End => 3,
            KeyboardKey::ArrowLeft if axis == Axis::Horizontal => 1,
            KeyboardKey::ArrowRight if axis == Axis::Horizontal => 2,
            KeyboardKey::ArrowUp if axis == Axis::Vertical => 1,
            KeyboardKey::ArrowDown if axis == Axis::Vertical => 2,
            _ => return Ok(None),
        };
        let headers: Vec<_> = self
            .order
            .iter()
            .copied()
            .filter(|id| {
                self.within(*id, list)
                    && self.enabled(*id)
                    && matches!(
                        self.tab_properties(*id),
                        Some(crate::tabs::Properties::Header { .. })
                    )
            })
            .collect();
        let Some(index) = headers.iter().position(|id| *id == target) else {
            return Ok(None);
        };
        let next = headers[match direction {
            0 => 0,
            1 => (index + headers.len() - 1) % headers.len(),
            2 => (index + 1) % headers.len(),
            _ => headers.len() - 1,
        }];
        let Some(crate::tabs::Properties::Header { key, select, .. }) = self.tab_properties(next)
        else {
            unreachable!()
        };
        let event = TabSelectEvent { key: key.clone() };
        let select = select.clone();
        let mut changed = self.focused != Some(next);
        self.change_focus(Some(next));
        changed |= self.reveal(next);
        if activation == TabActivation::Automatic
            && let Some(listener) = select
        {
            changed |= runtime.update(|cx| listener.dispatch(&event, cx))? == Dispatch::Handled;
        }
        Ok(Some(changed))
    }
}
