use super::*;
use crate::commands::{Action, Command, CommandId, CommandInfo, CommandStatus, invoke};
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
                        .filter(|a| {
                            matches(a) && (a.allow_in_modal || self.modal_allows(id)) && (a.live)()
                        })
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
                        (matches(&action)
                            && (action.allow_in_modal || self.active_modal().is_none())
                            && (action.live)())
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
        self.dispatch_command_id(runtime, CommandId::of::<C>())
    }
    /// Queries the current focused scope and application fallbacks. Preparation
    /// must have reconciled model changes; native activation does not erase focus.
    pub fn query_command<C: Command>(&self) -> Option<CommandInfo> {
        self.query_command_id(CommandId::of::<C>())
    }
    /// Queries an erased command identity without invoking its callback.
    pub fn query_command_id(&self, id: CommandId) -> Option<CommandInfo> {
        if !self.is_prepared() {
            return None;
        }
        self.resolve_command(|a| a.kind == id.0)
            .first()
            .map(Action::info)
    }
    pub(crate) fn dispatch_command_id(
        &self,
        runtime: &mut Runtime,
        id: CommandId,
    ) -> Result<CommandStatus, UiError> {
        runtime.is_dirty(&self.owner)?;
        if !self.is_prepared() {
            return Ok(CommandStatus::Unhandled);
        }
        for action in self.resolve_command(|a| a.kind == id.0) {
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
    #[cfg(all(
        feature = "native-menus",
        any(target_os = "macos", target_os = "windows")
    ))]
    pub(crate) fn shortcut_command(&self, shortcut: &crate::Shortcut) -> Option<CommandId> {
        self.resolve_command(|a| a.shortcuts.iter().any(|s| s == shortcut))
            .first()
            .map(|a| CommandId(a.kind))
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

#[cfg(feature = "native")]
impl<T: View> Ui<T> {
    pub(crate) fn native_command_info(&self, id: CommandId) -> Option<CommandInfo> {
        if let Some(info) = self.query_command_id(id) {
            return Some(info);
        }
        let text = self
            .focused
            .is_some_and(|id| self.enabled(id) && self.nodes[&id].editor.is_some());
        let editable = text
            && self.focused.is_some_and(|id| {
                matches!(
                    self.nodes[&id].element.kind,
                    ElementKind::TextInput {
                        read_only: false,
                        change: Some(_),
                        ..
                    }
                )
            });
        crate::commands::standard_info(
            id,
            true,
            text,
            editable,
            self.selected_text().is_some(),
            self.active_modal().is_some(),
        )
    }
    pub(crate) fn native_text_input(
        &mut self,
        runtime: &mut Runtime,
        event: crate::TextInputEvent,
        measure: &mut impl crate::TextMeasure,
    ) -> Result<bool, UiError> {
        // A native menu may temporarily deactivate the native window; its remembered
        // logical focus is still the intended editing target. Do not call set_active:
        // doing so would cancel composition/capture merely to execute a menu item.
        let active = self.active;
        self.active = true;
        let result = self.text_input(runtime, event, measure);
        self.active = active;
        result
    }
}
