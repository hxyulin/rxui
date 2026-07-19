//! Application-oriented widgets and models built on Astrelis UI.

#![warn(missing_docs)]

use astrelis_ui_core::{ElementHandle, Ui, UiError};
use astrelis_ui_widgets::{Menu, MenuItem};

mod controls;
mod icon;
mod shell;
mod validation;

pub use controls::{
    ComboBox, ComboBoxItem, FormSection, NumericField, NumericFieldOptions, RadioGroup, RadioOption,
};
pub use icon::{CommandButton, Icon, IconButton, IconError, IconView, icons};
pub use shell::{
    DialogAction, DialogActionRole, DialogError, DialogHost, DialogOptions, Toast, ToastAction,
    ToastHost, ToastId, ToastLevel, ToastQueue, Toolbar, ToolbarItem, ToolbarOptions,
};
pub use validation::{
    FieldValidation, FormValidation, ValidationIssue, ValidationResult, ValidationSeverity,
};

/// Application preference for selecting a visual theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemePreference {
    /// Follow the native window appearance.
    #[default]
    System,
    /// Always use the light theme.
    Light,
    /// Always use the dark theme.
    Dark,
}

/// Paired light and dark themes with system-appearance resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeSet {
    /// Light application theme.
    pub light: astrelis_ui_core::Theme,
    /// Dark application theme.
    pub dark: astrelis_ui_core::Theme,
}

impl ThemeSet {
    /// Resolves an application preference and optional native appearance.
    pub fn resolve(
        &self,
        preference: ThemePreference,
        system: Option<astrelis_platform::Theme>,
    ) -> &astrelis_ui_core::Theme {
        let dark = match preference {
            ThemePreference::Light => false,
            ThemePreference::Dark => true,
            ThemePreference::System => {
                matches!(system, Some(astrelis_platform::Theme::Dark))
            }
        };
        if dark { &self.dark } else { &self.light }
    }
}

impl Default for ThemeSet {
    fn default() -> Self {
        Self {
            light: astrelis_ui_core::Theme::light(),
            dark: astrelis_ui_core::Theme::dark(),
        }
    }
}
use astreon_app::{Command, CommandRegistry};

/// Re-exports of the lower-level Astrelis widget collection.
pub mod foundation {
    pub use astrelis_ui_widgets::*;
}

/// Creates a keyboard-accessible popup menu from registered commands.
///
/// Command enablement and messages are snapshotted when the menu is built.
/// Rebuild the menu after command presentation state changes.
pub fn command_menu<Message: Clone + 'static, T: 'static>(
    ui: &mut Ui<Message>,
    owner: ElementHandle<T>,
    commands: &CommandRegistry<Message>,
) -> Result<Menu, UiError> {
    Menu::new(
        ui,
        owner,
        commands
            .iter()
            .map(|command| MenuItem {
                label: command.label.clone(),
                message: command.message.clone(),
                enabled: command.enabled,
            })
            .collect(),
    )
}

/// One command returned by a palette search.
#[derive(Clone, Copy, Debug)]
pub struct CommandMatch<'a, Message> {
    /// Matching command.
    pub command: &'a Command<Message>,
    /// Search relevance; larger values sort first.
    pub score: u8,
}

/// Searches enabled commands for a command-palette query.
///
/// Exact identifier and label matches rank above prefix and substring matches.
pub fn search_commands<'a, Message>(
    commands: &'a CommandRegistry<Message>,
    query: &str,
) -> Vec<CommandMatch<'a, Message>> {
    let query = query.trim().to_lowercase();
    let mut matches = commands
        .iter()
        .filter(|command| command.enabled)
        .filter_map(|command| {
            let id = command.id.as_str().to_lowercase();
            let label = command.label.to_lowercase();
            let description = command
                .description
                .as_deref()
                .unwrap_or_default()
                .to_lowercase();
            let score = if query.is_empty() {
                1
            } else if label == query || id == query {
                100
            } else if label.starts_with(&query) || id.starts_with(&query) {
                75
            } else if label.contains(&query) || id.contains(&query) {
                50
            } else if description.contains(&query) {
                25
            } else {
                return None;
            };
            Some(CommandMatch { command, score })
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.command.label.cmp(&right.command.label))
    });
    matches
}

#[cfg(test)]
mod tests {
    use astreon_app::{CommandId, Shortcut};

    use super::*;

    #[test]
    fn palette_search_ranks_exact_before_description() {
        let mut commands = CommandRegistry::new();
        commands
            .register(
                Command::new(CommandId::new("file.save").unwrap(), "Save", 1)
                    .description("Write the document")
                    .shortcut(Shortcut::primary("s")),
            )
            .unwrap();
        commands
            .register(
                Command::new(CommandId::new("file.export").unwrap(), "Export", 2)
                    .description("Save a portable copy"),
            )
            .unwrap();

        let matches = search_commands(&commands, "save");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].command.id.as_str(), "file.save");
        assert!(matches[0].score > matches[1].score);
    }

    #[test]
    fn theme_preferences_override_the_system() {
        let themes = ThemeSet::default();
        assert_eq!(
            themes.resolve(
                ThemePreference::System,
                Some(astrelis_platform::Theme::Dark)
            ),
            &themes.dark
        );
        assert_eq!(
            themes.resolve(ThemePreference::Light, Some(astrelis_platform::Theme::Dark)),
            &themes.light
        );
    }
}
