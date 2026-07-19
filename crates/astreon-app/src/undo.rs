//! Reusable reversible application actions.

use std::any::Any;

use crate::{CommandId, CommandRegistry};

/// One reversible mutation of application state.
pub trait UndoAction<State, Error>: Any {
    /// Concise user-visible operation label.
    fn label(&self) -> &str;
    /// Applies or reapplies the mutation.
    fn redo(&mut self, state: &mut State) -> Result<(), Error>;
    /// Reverts the mutation.
    fn undo(&mut self, state: &mut State) -> Result<(), Error>;
    /// Absorbs a newer action, for example consecutive text edits.
    fn merge(&mut self, _newer: &dyn UndoAction<State, Error>) -> bool {
        false
    }
    /// Returns this action for type-aware merging.
    fn as_any(&self) -> &dyn Any;
}

/// Linear undo/redo history with clean-state and coalescing support.
pub struct UndoStack<State, Error> {
    actions: Vec<Box<dyn UndoAction<State, Error>>>,
    cursor: usize,
    clean: Option<usize>,
    limit: usize,
}

impl<State: 'static, Error: 'static> UndoStack<State, Error> {
    /// Creates an empty history. A zero limit means unlimited history.
    pub const fn new(limit: usize) -> Self {
        Self {
            actions: Vec::new(),
            cursor: 0,
            clean: Some(0),
            limit,
        }
    }

    /// Applies and records an action.
    pub fn execute<A>(&mut self, mut action: A, state: &mut State) -> Result<(), Error>
    where
        A: UndoAction<State, Error> + 'static,
    {
        action.redo(state)?;
        if self.cursor < self.actions.len() {
            self.actions.truncate(self.cursor);
            if self.clean.is_some_and(|clean| clean > self.cursor) {
                self.clean = None;
            }
        }
        if self.cursor > 0 && self.actions[self.cursor - 1].merge(&action) {
            self.clean = self.clean.filter(|clean| *clean != self.cursor);
            return Ok(());
        }
        self.actions.push(Box::new(action));
        self.cursor += 1;
        if self.limit > 0 && self.actions.len() > self.limit {
            let remove = self.actions.len() - self.limit;
            self.actions.drain(..remove);
            self.cursor -= remove;
            self.clean = self.clean.and_then(|clean| clean.checked_sub(remove));
        }
        Ok(())
    }

    /// Reverts the latest applied action.
    pub fn undo(&mut self, state: &mut State) -> Result<bool, Error> {
        if self.cursor == 0 {
            return Ok(false);
        }
        self.actions[self.cursor - 1].undo(state)?;
        self.cursor -= 1;
        Ok(true)
    }

    /// Reapplies the next reverted action.
    pub fn redo(&mut self, state: &mut State) -> Result<bool, Error> {
        if self.cursor == self.actions.len() {
            return Ok(false);
        }
        self.actions[self.cursor].redo(state)?;
        self.cursor += 1;
        Ok(true)
    }

    /// Marks the current position as saved.
    pub const fn mark_clean(&mut self) {
        self.clean = Some(self.cursor);
    }
    /// Returns whether the current position is the saved position.
    pub fn is_clean(&self) -> bool {
        self.clean == Some(self.cursor)
    }
    /// Returns whether an undo is available.
    pub const fn can_undo(&self) -> bool {
        self.cursor > 0
    }
    /// Returns whether a redo is available.
    pub fn can_redo(&self) -> bool {
        self.cursor < self.actions.len()
    }
    /// Returns the next undo label.
    pub fn undo_label(&self) -> Option<&str> {
        self.cursor.checked_sub(1).map(|i| self.actions[i].label())
    }
    /// Returns the next redo label.
    pub fn redo_label(&self) -> Option<&str> {
        self.actions.get(self.cursor).map(|a| a.label())
    }
    /// Removes all history and marks the stack clean.
    pub fn clear(&mut self) {
        self.actions.clear();
        self.cursor = 0;
        self.clean = Some(0);
    }
}

/// Conventional `edit.undo` command identity.
pub fn undo_command_id() -> CommandId {
    CommandId::new("edit.undo").expect("static command id")
}
/// Conventional `edit.redo` command identity.
pub fn redo_command_id() -> CommandId {
    CommandId::new("edit.redo").expect("static command id")
}

/// Synchronizes conventional command labels and enablement with a history.
pub fn sync_undo_commands<State: 'static, Error: 'static, Message>(
    commands: &mut CommandRegistry<Message>,
    stack: &UndoStack<State, Error>,
) {
    if let Some(command) = commands.get_mut(&undo_command_id()) {
        command.enabled = stack.can_undo();
        command.label = stack
            .undo_label()
            .map_or_else(|| "Undo".into(), |label| format!("Undo {label}"));
    }
    if let Some(command) = commands.get_mut(&redo_command_id()) {
        command.enabled = stack.can_redo();
        command.label = stack
            .redo_label()
            .map_or_else(|| "Redo".into(), |label| format!("Redo {label}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Add(i32);
    impl UndoAction<i32, ()> for Add {
        fn label(&self) -> &str {
            "Change value"
        }
        fn redo(&mut self, value: &mut i32) -> Result<(), ()> {
            *value += self.0;
            Ok(())
        }
        fn undo(&mut self, value: &mut i32) -> Result<(), ()> {
            *value -= self.0;
            Ok(())
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn history_truncates_redo_and_tracks_clean_state() {
        let mut value = 0;
        let mut stack = UndoStack::new(0);
        stack.execute(Add(2), &mut value).unwrap();
        stack.mark_clean();
        stack.execute(Add(3), &mut value).unwrap();
        assert_eq!(value, 5);
        assert!(stack.undo(&mut value).unwrap());
        assert_eq!(value, 2);
        assert!(stack.is_clean());
        stack.execute(Add(4), &mut value).unwrap();
        assert!(!stack.can_redo());
    }
}
