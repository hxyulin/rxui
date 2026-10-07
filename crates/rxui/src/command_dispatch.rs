use super::*;
use crate::commands::{Action, Command, CommandStatus, invoke};
use std::any::TypeId;
impl<T: View> Ui<T> {
    fn resolve_command(&self, matches: impl Fn(&Action) -> bool) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut current = self.focused.or(self.root);
        while let Some(id) = current {
            let node = &self.nodes[&id];
            // Modal background ancestors do not supply commands to a dialog.
            if self.input_available_unconfined(id)
                && let Some(input) = &node.element.input
            {
                actions.extend(
                    input
                        .commands
                        .iter()
                        .filter(|a| (a.allow_in_modal || self.modal_allows(id)) && matches(a))
                        .cloned(),
                );
            }
            current = node.parent;
        }
        if let Some(runtime) = self.runtime.upgrade() {
            actions.extend(
                runtime
                    .commands
                    .borrow()
                    .iter()
                    .rev()
                    .filter_map(|r| r.upgrade())
                    .filter_map(|r| {
                        let action = r.action.borrow();
                        ((action.allow_in_modal || self.active_modal().is_none())
                            && matches(&action))
                        .then(|| action.clone())
                    }),
            );
        }
        actions
    }
    /// Resolves a typed action from focus through ancestors, then application
    /// registrations. Uses the handler's bound payload; invoke a CommandAction
    /// directly when a button/menu targets a particular action instance.
    pub fn dispatch_command<C: Command>(
        &self,
        runtime: &mut Runtime,
    ) -> Result<CommandStatus, UiError> {
        runtime.is_dirty(&self.owner)?;
        if !self.is_prepared() || !self.active {
            return Ok(CommandStatus::Unhandled);
        }
        for action in self.resolve_command(|a| a.kind == TypeId::of::<C>()) {
            let status = runtime.update(|cx| {
                cx.dispatch_mount = Some(self.owner.id());
                invoke(&action, cx)
            })?;
            if status != CommandStatus::Unhandled {
                return Ok(status);
            }
        }
        Ok(CommandStatus::Unhandled)
    }
    pub(super) fn command_key(
        &self,
        runtime: &mut Runtime,
        event: &crate::KeyEvent,
    ) -> Result<crate::InputResult, UiError> {
        for action in self.resolve_command(|a| a.shortcuts.iter().any(|s| s.matches(event))) {
            if event.repeat && !action.repeat {
                return Ok(crate::InputResult {
                    changed: false,
                    default_prevented: true,
                });
            }
            let status = runtime.update(|cx| {
                cx.dispatch_mount = Some(self.owner.id());
                invoke(&action, cx)
            })?;
            if status != CommandStatus::Unhandled {
                return Ok(crate::InputResult {
                    changed: status == CommandStatus::Handled,
                    default_prevented: true,
                });
            }
        }
        Ok(Default::default())
    }
}
