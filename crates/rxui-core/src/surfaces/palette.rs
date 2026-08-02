//! Keyboard-first command palettes.

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::Alignment;

use crate::{
    ColorRole, ContainerStyle, FrameStyle, Space, View, button, column, column_with, text_field,
    views,
};

/// One command-palette result.
#[derive(Clone)]
pub struct CommandItem<Action> {
    /// Stable command identity.
    pub id: String,
    /// User-visible label.
    pub label: String,
    /// Optional description.
    pub description: Option<String>,
    /// Typed invocation action.
    pub action: Action,
    /// Whether the command accepts invocation.
    pub enabled: bool,
}

/// Controlled keyboard actions for a command palette.
#[derive(Clone)]
pub struct CommandPaletteNavigation<Action> {
    /// Close the palette without invoking a command.
    pub dismiss: Action,
    /// Select the previous visible command.
    pub previous: Action,
    /// Select the next visible command.
    pub next: Action,
}

/// Command rows one palette shows at once.
const COMMAND_PALETTE_MATCHES: usize = 12;

/// Selects the commands a query matches, in declaration order.
///
/// Matching is case-insensitive over label and description, ignores
/// surrounding whitespace, and keeps at most [`COMMAND_PALETTE_MATCHES`] rows.
fn filter_commands<'a, Action>(
    commands: &'a [CommandItem<Action>],
    query: &str,
) -> Vec<&'a CommandItem<Action>> {
    let query = query.trim().to_lowercase();
    commands
        .iter()
        .filter(|command| {
            query.is_empty()
                || command.label.to_lowercase().contains(&query)
                || command
                    .description
                    .as_deref()
                    .is_some_and(|value| value.to_lowercase().contains(&query))
        })
        .take(COMMAND_PALETTE_MATCHES)
        .collect()
}

/// Keeps a caller-owned selection inside the visible match list.
///
/// An empty match list has no selectable row; the returned index is then only
/// meaningful as a lookup that misses.
const fn clamp_selection(selected: usize, matches: usize) -> usize {
    if selected >= matches {
        matches.saturating_sub(1)
    } else {
        selected
    }
}

/// Builds a controlled keyboard-first command palette surface.
pub fn command_palette<Action: Clone + 'static>(
    open: bool,
    query: &str,
    commands: &[CommandItem<Action>],
    selected: usize,
    on_query: impl Fn(String) -> Action + 'static,
    navigation: CommandPaletteNavigation<Action>,
) -> View<Action> {
    let matches = filter_commands(commands, query);
    let selected = clamp_selection(selected, matches.len());
    let submit = matches
        .get(selected)
        .filter(|command| command.enabled)
        .map(|command| command.action.clone())
        .unwrap_or_else(|| navigation.dismiss.clone());
    column_with(
        ContainerStyle::new()
            .gap(Space::Sm)
            .padding(Space::Lg)
            .background(ColorRole::Surface),
        (
            text_field("Command search", query, on_query),
            column(views(matches.into_iter().enumerate().map(
                |(index, command)| {
                    button(
                        format!(
                            "{}{}",
                            if index == selected { "› " } else { "  " },
                            command.label
                        ),
                        command.action.clone(),
                    )
                    .enabled(command.enabled)
                    .key(command.id.clone())
                },
            ))),
        ),
    )
    .frame(
        FrameStyle::new()
            .min(LogicalSize::new(420.0, 0.0))
            .max(LogicalSize::new(640.0, 560.0)),
    )
    .visible(open)
    .focus_scope(open)
    .command_navigation(navigation.previous, navigation.next, submit)
    .dismiss_on_escape(navigation.dismiss)
    .aligned(Alignment::Top, Space::Xl)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(id: &str, label: &str, description: Option<&str>) -> CommandItem<()> {
        CommandItem {
            id: id.into(),
            label: label.into(),
            description: description.map(Into::into),
            action: (),
            enabled: true,
        }
    }

    fn ids(matches: &[&CommandItem<()>]) -> Vec<String> {
        matches.iter().map(|command| command.id.clone()).collect()
    }

    #[test]
    fn an_empty_query_matches_every_command() {
        let commands = [
            command("open", "Open File", None),
            command("save", "Save File", None),
        ];
        assert_eq!(ids(&filter_commands(&commands, "")), ["open", "save"]);
        assert_eq!(ids(&filter_commands(&commands, "   ")), ["open", "save"]);
    }

    #[test]
    fn commands_match_labels_and_descriptions_case_insensitively() {
        let commands = [
            command("open", "Open File", None),
            command("save", "Save File", Some("Write the document to disk")),
            command("quit", "Quit", None),
        ];
        assert_eq!(ids(&filter_commands(&commands, "FILE")), ["open", "save"]);
        assert_eq!(ids(&filter_commands(&commands, "  disk ")), ["save"]);
        assert_eq!(ids(&filter_commands(&commands, "qui")), ["quit"]);
        assert!(filter_commands(&commands, "nothing").is_empty());
    }

    #[test]
    fn matches_keep_declaration_order_and_stop_at_the_row_budget() {
        let commands = (0..40)
            .map(|index| {
                let id = format!("command-{index}");
                command(&id, &format!("Command {index}"), None)
            })
            .collect::<Vec<_>>();
        let matches = filter_commands(&commands, "command");
        assert_eq!(matches.len(), COMMAND_PALETTE_MATCHES);
        assert_eq!(matches[0].id, "command-0");
        assert_eq!(matches[COMMAND_PALETTE_MATCHES - 1].id, "command-11");
    }

    #[test]
    fn selection_inside_the_match_list_is_left_alone() {
        assert_eq!(clamp_selection(0, 3), 0);
        assert_eq!(clamp_selection(2, 3), 2);
    }

    #[test]
    fn selection_past_the_last_match_lands_on_it() {
        assert_eq!(clamp_selection(3, 3), 2);
        assert_eq!(clamp_selection(usize::MAX, 3), 2);
    }

    #[test]
    fn selection_in_an_empty_match_list_cannot_address_a_row() {
        let selected = clamp_selection(7, 0);
        assert_eq!(selected, 0);
        let commands: [CommandItem<()>; 0] = [];
        assert!(filter_commands(&commands, "").get(selected).is_none());
    }
}
