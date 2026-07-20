//! Keyboard-first command palette surface.

use std::rc::Rc;

use astrelis_platform::{ElementState, Key, NamedKey};
use astrelis_ui_core::{
    Button, Column, ElementHandle, EventFilter, FocusScopeOptions, Insets, LayoutStyle, Length,
    Overlay, OverlayAlignment, OverlayOptions, OverlaySide, RoutedEventKind, SemanticRole,
    TextField, Ui, UiError, Visibility,
};
use rxui_app::{CommandId, CommandRegistry};

use crate::search_commands;

/// Application-visible command-palette interaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandPaletteEvent {
    /// Search text changed.
    QueryChanged(String),
    /// Move the controlled result selection by a signed number of rows.
    Navigate(i32),
    /// Invoke a registered command by stable identity.
    Invoke(CommandId),
    /// Invoke the currently selected filtered result.
    InvokeSelected,
    /// Close the palette without invoking a command.
    Dismiss,
}

/// Controlled command-palette state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandPaletteState {
    /// Whether the palette is visible.
    pub open: bool,
    /// Current search query.
    pub query: String,
    /// Selected result index after filtering.
    pub selected: usize,
}

impl CommandPaletteState {
    /// Applies navigation while clamping to the available result count.
    pub fn navigate(&mut self, delta: i32, result_count: usize) {
        if result_count == 0 {
            self.selected = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta as isize)
            .min(result_count - 1);
    }
}

/// Modal, focus-restoring command search surface.
pub struct CommandPalette<Message> {
    overlay: ElementHandle<Overlay>,
    field: ElementHandle<TextField>,
    results: ElementHandle<Column>,
    rows: Vec<ElementHandle<Button>>,
    map_event: Rc<dyn Fn(CommandPaletteEvent) -> Message>,
}

impl<Message: 'static> CommandPalette<Message> {
    /// Attaches a hidden palette to the viewport.
    pub fn new(
        ui: &mut Ui<Message>,
        map_event: impl Fn(CommandPaletteEvent) -> Message + 'static,
    ) -> Result<Self, UiError> {
        let overlay = ui.add_overlay(
            ui.root(),
            OverlayOptions {
                side: OverlaySide::Center,
                alignment: OverlayAlignment::Center,
                z_index: 1_000,
                focus: FocusScopeOptions {
                    trapped: true,
                    autofocus: true,
                    restore_focus: true,
                },
                ..Default::default()
            },
        )?;
        ui.set_semantic_role(overlay, SemanticRole::Dialog)?;
        ui.set_semantic_description(overlay, Some("Search and run application commands".into()))?;
        ui.set_visibility(overlay, Visibility::Collapsed)?;
        ui.set_layout(
            overlay,
            LayoutStyle {
                width: Length::Px(520.0),
                max_width: Length::Percent(0.9),
                ..Default::default()
            },
        )?;
        let padding = ui.add_padding(overlay, Insets::all(12.0))?;
        let content = ui.add_column(padding)?;
        let field = ui.add_text_field(content, "")?;
        ui.set_semantic_description(field, Some("Command search".into()))?;
        let results = ui.add_column(content)?;
        ui.set_semantic_role(results, SemanticRole::List)?;
        let mapper = Rc::new(map_event);
        let query_mapper = mapper.clone();
        ui.listen(
            field,
            None,
            EventFilter::ValueChanged,
            move |context, event| {
                if let RoutedEventKind::TextChanged(query) = &event.kind {
                    context.emit(query_mapper(CommandPaletteEvent::QueryChanged(
                        query.clone(),
                    )));
                }
            },
        )?;
        let keyboard_mapper = mapper.clone();
        ui.listen(field, None, EventFilter::Keyboard, move |context, event| {
            if let RoutedEventKind::Keyboard(input) = &event.kind
                && input.state == ElementState::Pressed
            {
                let event = match &input.logical_key {
                    Key::Named(NamedKey::Other(key)) if key == "ArrowUp" => {
                        Some(CommandPaletteEvent::Navigate(-1))
                    }
                    Key::Named(NamedKey::Other(key)) if key == "ArrowDown" => {
                        Some(CommandPaletteEvent::Navigate(1))
                    }
                    Key::Named(NamedKey::Other(key)) if key == "Home" => {
                        Some(CommandPaletteEvent::Navigate(i32::MIN))
                    }
                    Key::Named(NamedKey::Other(key)) if key == "End" => {
                        Some(CommandPaletteEvent::Navigate(i32::MAX))
                    }
                    Key::Named(NamedKey::Enter) => Some(CommandPaletteEvent::InvokeSelected),
                    Key::Named(NamedKey::Escape) => Some(CommandPaletteEvent::Dismiss),
                    _ => None,
                };
                if let Some(event) = event {
                    context.emit(keyboard_mapper(event));
                    context.prevent_default();
                }
            }
        })?;
        Ok(Self {
            overlay,
            field,
            results,
            rows: Vec::new(),
            map_event: mapper,
        })
    }

    /// Returns the viewport-hosted palette surface.
    pub const fn overlay(&self) -> ElementHandle<Overlay> {
        self.overlay
    }

    /// Reconciles visibility, query text, result rows, and controlled selection.
    pub fn sync(
        &mut self,
        ui: &mut Ui<Message>,
        commands: &CommandRegistry<Message>,
        state: &CommandPaletteState,
    ) -> Result<(), UiError> {
        ui.set_visibility(
            self.overlay,
            if state.open {
                Visibility::Visible
            } else {
                Visibility::Collapsed
            },
        )?;
        if !state.open {
            return Ok(());
        }
        ui.set_text(self.field, &state.query)?;
        for row in self.rows.drain(..) {
            ui.remove(row)?;
        }
        let matches = search_commands(commands, &state.query);
        for (index, matched) in matches.into_iter().take(12).enumerate() {
            let shortcut = matched
                .command
                .shortcut
                .as_ref()
                .map(|shortcut| format!(" — {}", shortcut.display_label()))
                .unwrap_or_default();
            let marker = if index == state.selected {
                "› "
            } else {
                "  "
            };
            let row = ui.add_button(
                self.results,
                format!("{marker}{}{shortcut}", matched.command.label),
            )?;
            ui.set_semantic_role(row, SemanticRole::ListItem)?;
            ui.set_semantic_selected(row, Some(index == state.selected))?;
            if let Some(description) = &matched.command.description {
                ui.set_semantic_description(row, Some(description.clone()))?;
            }
            let id = matched.command.id.clone();
            let mapper = self.map_event.clone();
            ui.listen(row, None, EventFilter::Activate, move |context, _| {
                context.emit(mapper(CommandPaletteEvent::Invoke(id.clone())))
            })?;
            self.rows.push(row);
        }
        ui.focus(self.field)
    }

    /// Returns enabled command identities in current display order.
    pub fn matches<Message2>(commands: &CommandRegistry<Message2>, query: &str) -> Vec<CommandId> {
        search_commands(commands, query)
            .into_iter()
            .take(12)
            .map(|matched| matched.command.id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_clamps_and_empty_results_reset_selection() {
        let mut state = CommandPaletteState {
            open: true,
            query: String::new(),
            selected: 2,
        };
        state.navigate(10, 4);
        assert_eq!(state.selected, 3);
        state.navigate(-2, 4);
        assert_eq!(state.selected, 1);
        state.navigate(i32::MAX, 4);
        assert_eq!(state.selected, 3);
        state.navigate(i32::MIN, 4);
        assert_eq!(state.selected, 0);
        state.navigate(1, 0);
        assert_eq!(state.selected, 0);
    }
}
