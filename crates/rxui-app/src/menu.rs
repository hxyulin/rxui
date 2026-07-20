//! Backend-neutral application menu models.

use std::{error::Error, fmt};

use crate::{CommandId, CommandRegistry};

/// A complete application menu bar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuBar {
    /// Top-level menus in display order.
    pub menus: Vec<Menu>,
}

impl MenuBar {
    /// Creates an empty menu bar.
    pub const fn new() -> Self {
        Self { menus: Vec::new() }
    }

    /// Appends a top-level menu.
    pub fn menu(mut self, menu: Menu) -> Self {
        self.menus.push(menu);
        self
    }

    /// Validates structure and command references.
    pub fn validate<Message>(&self, commands: &CommandRegistry<Message>) -> Result<(), MenuError> {
        if self.menus.is_empty() {
            return Err(MenuError::new("a menu bar requires at least one menu"));
        }
        for menu in &self.menus {
            menu.validate(commands, 0)?;
        }
        Ok(())
    }
}

/// One labelled menu or submenu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    /// User-visible label.
    pub label: String,
    /// Entries in display order.
    pub entries: Vec<MenuEntry>,
}

impl Menu {
    /// Creates an empty labelled menu.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            entries: Vec::new(),
        }
    }

    /// Appends an entry.
    pub fn entry(mut self, entry: MenuEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Appends a command reference.
    pub fn command(self, id: CommandId) -> Self {
        self.entry(MenuEntry::Command(id))
    }

    /// Appends a native role.
    pub fn role(self, role: MenuRole) -> Self {
        self.entry(MenuEntry::Role(role))
    }

    /// Appends a separator.
    pub fn separator(self) -> Self {
        self.entry(MenuEntry::Separator)
    }

    fn validate<Message>(
        &self,
        commands: &CommandRegistry<Message>,
        depth: usize,
    ) -> Result<(), MenuError> {
        if self.label.trim().is_empty() {
            return Err(MenuError::new("menu labels cannot be empty"));
        }
        if self.entries.is_empty() {
            return Err(MenuError::new(format!(
                "menu `{}` cannot be empty",
                self.label
            )));
        }
        if depth >= 8 {
            return Err(MenuError::new(
                "menus cannot be nested more than eight levels",
            ));
        }
        if matches!(self.entries.first(), Some(MenuEntry::Separator))
            || matches!(self.entries.last(), Some(MenuEntry::Separator))
            || self
                .entries
                .windows(2)
                .any(|pair| matches!(pair, [MenuEntry::Separator, MenuEntry::Separator]))
        {
            return Err(MenuError::new(format!(
                "menu `{}` has a leading, trailing, or repeated separator",
                self.label
            )));
        }
        for entry in &self.entries {
            match entry {
                MenuEntry::Command(id) if commands.get(id).is_none() => {
                    return Err(MenuError::new(format!(
                        "menu `{}` references unknown command `{id}`",
                        self.label
                    )));
                }
                MenuEntry::Submenu(menu) => menu.validate(commands, depth + 1)?,
                _ => {}
            }
        }
        Ok(())
    }
}

/// One entry in an application menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    /// Presentation and activation are resolved through a command registry.
    Command(CommandId),
    /// Conventional behavior implemented by the operating system.
    Role(MenuRole),
    /// Nested menu.
    Submenu(Menu),
    /// Native separator.
    Separator,
}

/// Conventional native menu behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuRole {
    /// Show application information.
    About,
    /// Copy the current selection.
    Copy,
    /// Cut the current selection.
    Cut,
    /// Paste clipboard contents.
    Paste,
    /// Select all applicable content.
    SelectAll,
    /// Undo the most recent operation.
    Undo,
    /// Redo the most recently undone operation.
    Redo,
    /// Minimize the active window.
    Minimize,
    /// Maximize the active window.
    Maximize,
    /// Close the active window.
    CloseWindow,
    /// Quit the application.
    Quit,
    /// Enter or leave fullscreen; currently macOS-only.
    Fullscreen,
    /// Hide the application; currently macOS-only.
    Hide,
    /// Hide other applications; currently macOS-only.
    HideOthers,
    /// Show all application windows; currently macOS-only.
    ShowAll,
    /// System Services submenu; currently macOS-only.
    Services,
    /// Bring all application windows forward; currently macOS-only.
    BringAllToFront,
}

/// Invalid application menu structure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuError(String);

impl MenuError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for MenuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for MenuError {}

#[cfg(test)]
mod tests {
    use crate::Command;

    use super::*;

    #[test]
    fn validates_command_references_and_separators() {
        let id = CommandId::new("file.save").unwrap();
        let mut commands = CommandRegistry::new();
        commands
            .register(Command::new(id.clone(), "Save", ()))
            .unwrap();
        assert!(
            MenuBar::new()
                .menu(Menu::new("File").command(id).separator())
                .validate(&commands)
                .is_err()
        );
    }
}
