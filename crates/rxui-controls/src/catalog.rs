//! Component-native composite controls and application surfaces.

use std::{fmt::Display, str::FromStr};

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::Alignment;

use rxui_core::{
    ButtonStyle, ButtonVariant, ColorRole, ContainerStyle, FrameStyle, Icon, IconButtonStyle,
    Space, StackStyle, View, ViewKey, button, button_with, column, column_with, icon_button_with,
    label, panel, row, row_with, spacer, stack_with, text_field, views,
};

/// One controlled radio-group option.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice<Value> {
    /// Stable choice identity, independent of position in the group.
    pub id: ViewKey,
    /// Domain value emitted when selected.
    pub value: Value,
    /// User-visible label.
    pub label: String,
    /// Whether the choice accepts interaction.
    pub enabled: bool,
}

impl<Value> Choice<Value> {
    /// Creates an enabled choice.
    pub fn new(id: impl Into<ViewKey>, value: Value, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            value,
            label: label.into(),
            enabled: true,
        }
    }

    /// Changes interaction enablement.
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Builds a controlled, keyed single-selection group.
pub fn radio_group<Action, Value>(
    choices: &[Choice<Value>],
    selected: Option<&Value>,
    on_selected: impl Fn(Value) -> Action + Clone + 'static,
) -> View<Action>
where
    Action: Clone + 'static,
    Value: Clone + PartialEq + 'static,
{
    column(views(choices.iter().map(|choice| {
        let marker = if selected == Some(&choice.value) {
            "●"
        } else {
            "○"
        };
        let value = choice.value.clone();
        let on_selected = on_selected.clone();
        button(format!("{marker} {}", choice.label), on_selected(value))
            .enabled(choice.enabled)
            .key(choice.id.clone())
    })))
}

/// One controlled combo-box option.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComboOption<Value> {
    /// Stable option identity, independent of position in the list.
    pub id: ViewKey,
    /// Domain value.
    pub value: Value,
    /// User-visible label.
    pub label: String,
    /// Whether the option accepts selection.
    pub enabled: bool,
}

impl<Value> ComboOption<Value> {
    /// Creates an enabled option.
    pub fn new(id: impl Into<ViewKey>, value: Value, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            value,
            label: label.into(),
            enabled: true,
        }
    }
}

/// Builds a controlled combo box with an inline popup surface.
pub fn combo_box<Action, Value>(
    label_text: impl Into<String>,
    options: &[ComboOption<Value>],
    selected: Option<&Value>,
    open: bool,
    toggle: Action,
    on_selected: impl Fn(Value) -> Action + Clone + 'static,
) -> View<Action>
where
    Action: Clone + 'static,
    Value: Clone + PartialEq + 'static,
{
    let selected_label = options
        .iter()
        .find(|option| selected == Some(&option.value))
        .map(|option| option.label.as_str())
        .unwrap_or("Select…");
    let popup = if open {
        column_with(
            ContainerStyle::new()
                .gap(Space::Xs)
                .padding(Space::Xs)
                .background(ColorRole::Surface),
            views(options.iter().map(|option| {
                let value = option.value.clone();
                let on_selected = on_selected.clone();
                button(option.label.clone(), on_selected(value))
                    .enabled(option.enabled)
                    .key(option.id.clone())
            })),
        )
    } else {
        column(Vec::new())
    };
    column((label(label_text), button(selected_label, toggle), popup))
}

/// Builds a controlled numeric field and reports parsing failures explicitly.
pub fn numeric_field<Action, Number>(
    label_text: impl Into<String>,
    value: Number,
    on_changed: impl Fn(Result<Number, String>) -> Action + 'static,
) -> View<Action>
where
    Action: 'static,
    Number: Display + FromStr + 'static,
    Number::Err: Display,
{
    text_field(label_text, value.to_string(), move |text| {
        on_changed(text.parse().map_err(|error: Number::Err| error.to_string()))
    })
}

/// A titled form region.
pub fn form_section<Action: 'static>(
    title: impl Into<String>,
    content: View<Action>,
) -> View<Action> {
    column_with(
        ContainerStyle::new()
            .gap(Space::Sm)
            .padding(Space::Md)
            .background(ColorRole::Surface),
        (label(title), content),
    )
}

/// One toolbar entry.
#[derive(Clone)]
pub enum ToolbarItem<Action> {
    /// Activatable command.
    Command {
        /// Stable command identity, independent of toolbar position.
        id: ViewKey,
        /// User-visible label.
        label: String,
        /// Typed action.
        action: Action,
        /// Whether the command accepts interaction.
        enabled: bool,
        /// Semantic presentation.
        variant: ButtonVariant,
    },
    /// Vector-icon command with compact or labeled presentation.
    IconCommand {
        /// Stable command identity, independent of toolbar position.
        id: ViewKey,
        /// Monochrome vector glyph.
        icon: Icon,
        /// Accessible label, optionally also painted.
        label: String,
        /// Typed action.
        action: Action,
        /// Whether the command accepts interaction.
        enabled: bool,
        /// Icon-button presentation.
        style: IconButtonStyle,
    },
    /// Visual separator.
    Separator,
    /// Fixed logical spacing.
    Space(f32),
}

/// Builds a keyed command toolbar.
pub fn toolbar<Action: Clone + 'static>(items: &[ToolbarItem<Action>]) -> View<Action> {
    // Separators and spacers are interchangeable and retain no interaction
    // state, so numbering them among themselves is a canonical identity: a
    // command inserted anywhere still leaves every decoration key alone.
    let mut decorations = 0usize;
    let keys = items
        .iter()
        .map(|item| match item {
            ToolbarItem::Command { id, .. } | ToolbarItem::IconCommand { id, .. } => {
                ViewKey::new(format!("command-{id}"))
            }
            ToolbarItem::Separator | ToolbarItem::Space(_) => {
                decorations += 1;
                ViewKey::new(format!("decoration-{}", decorations - 1))
            }
        })
        .collect::<Vec<_>>();
    row_with(
        ContainerStyle::new()
            .gap(Space::Xs)
            .padding(Space::Xs)
            .background(ColorRole::Surface),
        views(items.iter().zip(keys).map(|(item, key)| {
            let view = match item {
                ToolbarItem::Command {
                    label,
                    action,
                    enabled,
                    variant,
                    ..
                } => button_with(
                    label.clone(),
                    action.clone(),
                    ButtonStyle::standard().variant(*variant),
                )
                .enabled(*enabled),
                ToolbarItem::IconCommand {
                    icon,
                    label,
                    action,
                    enabled,
                    style,
                    ..
                } => icon_button_with(icon.clone(), label.clone(), action.clone(), *style)
                    .enabled(*enabled),
                ToolbarItem::Separator => {
                    panel(LogicalSize::new(1.0, 24.0), ColorRole::Muted, None)
                }
                ToolbarItem::Space(width) => spacer(LogicalSize::new((*width).max(0.0), 1.0)),
            };
            view.key(key)
        })),
    )
}

/// One modal-dialog action.
///
/// The label doubles as the action's identity for keyed reconciliation, so
/// two actions in one dialog must not share a label.
#[derive(Clone)]
pub struct DialogAction<Action> {
    /// User-visible label.
    pub label: String,
    /// Typed action.
    pub action: Action,
    /// Semantic presentation.
    pub variant: ButtonVariant,
}

/// Builds a controlled modal surface over a background view.
pub fn dialog<Action: Clone + 'static>(
    open: bool,
    title: impl Into<String>,
    background: View<Action>,
    content: View<Action>,
    on_dismiss: Action,
    actions: &[DialogAction<Action>],
) -> View<Action> {
    let modal = column_with(
        ContainerStyle::new()
            .gap(Space::Md)
            .padding(Space::Lg)
            .background(ColorRole::Surface),
        (
            label(title),
            content,
            row(views(actions.iter().map(|action| {
                button_with(
                    action.label.clone(),
                    action.action.clone(),
                    ButtonStyle::standard().variant(action.variant),
                )
                .key(action.label.clone())
            }))),
        ),
    )
    .frame(
        FrameStyle::new()
            .max(LogicalSize::new(560.0, 640.0))
            .min(LogicalSize::new(360.0, 0.0)),
    )
    .visible(open)
    .focus_scope(open)
    .dismiss_on_escape(on_dismiss)
    .aligned(Alignment::Center, Space::Xl);
    stack_with(
        StackStyle::new().background(ColorRole::Background),
        (
            background.enabled(!open).frame(FrameStyle::new().grow(1.0)),
            modal,
        ),
    )
}

/// Toast urgency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastLevel {
    /// Informational status.
    #[default]
    Info,
    /// Successful operation.
    Success,
    /// Recoverable warning.
    Warning,
    /// Failed operation.
    Error,
}

/// One controlled toast notification.
#[derive(Clone)]
pub struct Toast<Action> {
    /// Stable identity.
    pub id: u64,
    /// User-visible message.
    pub message: String,
    /// Urgency.
    pub level: ToastLevel,
    /// Optional action label and payload.
    pub action: Option<(String, Action)>,
}

/// Builds keyed toast surfaces.
pub fn toasts<Action: Clone + 'static>(items: &[Toast<Action>]) -> View<Action> {
    column_with(
        ContainerStyle::new().gap(Space::Sm).padding(Space::Sm),
        views(items.iter().map(|toast| {
            let role = match toast.level {
                ToastLevel::Info | ToastLevel::Success => ColorRole::Surface,
                ToastLevel::Warning => ColorRole::Accent,
                ToastLevel::Error => ColorRole::Danger,
            };
            let action = toast
                .action
                .as_ref()
                .map(|(label, action)| button(label.clone(), action.clone()));
            let children = std::iter::once(label(toast.message.clone()))
                .chain(action)
                .collect::<Vec<_>>();
            column_with(
                ContainerStyle::new()
                    .gap(Space::Xs)
                    .padding(Space::Md)
                    .background(role),
                children,
            )
            .key(toast.id)
        })),
    )
    .frame(FrameStyle::new().max(LogicalSize::new(360.0, 640.0)))
    .aligned(Alignment::TopTrailing, Space::Md)
}

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
    use astrelis_ui_next::{NodeId, SemanticNode};
    use rxui_core::{Component, ComponentContext, ComponentHost, Theme};

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

    fn focused(host: &ComponentHost<impl Component>) -> SemanticNode {
        host.ui()
            .semantic_snapshot()
            .into_iter()
            .find(|node| node.focused)
            .expect("a focused node")
    }

    fn node_labeled(host: &ComponentHost<impl Component>, suffix: &str) -> NodeId {
        host.ui()
            .semantic_snapshot()
            .into_iter()
            .find(|node| node.data.label.ends_with(suffix))
            .map(|node| node.id)
            .unwrap_or_else(|| panic!("no node labeled {suffix}"))
    }

    struct RadioForm {
        choices: Vec<Choice<&'static str>>,
    }

    impl Component for RadioForm {
        type Action = &'static str;
        type Effect = ();

        fn update(&mut self, _action: Self::Action, _context: &mut ComponentContext<'_, ()>) {}

        fn view(&self, _theme: &Theme) -> View<Self::Action> {
            radio_group(&self.choices, None, |value| value)
        }
    }

    #[test]
    fn radio_choices_keep_their_retained_identity_across_an_insertion() {
        let mut host = ComponentHost::new(
            RadioForm {
                choices: vec![
                    Choice::new("beta", "beta", "Beta"),
                    Choice::new("gamma", "gamma", "Gamma"),
                ],
            },
            LogicalSize::new(320.0, 240.0),
            Theme::dark(),
        )
        .unwrap();
        let beta = node_labeled(&host, "Beta");
        host.ui_mut().set_focus(Some(beta)).unwrap();
        host.refresh().unwrap();
        assert_eq!(focused(&host).id, beta);

        host.component_mut()
            .choices
            .insert(0, Choice::new("alpha", "alpha", "Alpha"));
        host.refresh().unwrap();

        // Position-keyed children would hand Beta's retained control - and its
        // focus - to the newly inserted Alpha.
        let focused = focused(&host);
        assert_eq!(focused.id, beta);
        assert!(focused.data.label.ends_with("Beta"), "{:?}", focused.data);
        assert_eq!(node_labeled(&host, "Beta"), beta);
        assert_ne!(node_labeled(&host, "Alpha"), beta);
    }

    struct Toolbar {
        commands: Vec<&'static str>,
    }

    impl Component for Toolbar {
        type Action = &'static str;
        type Effect = ();

        fn update(&mut self, _action: Self::Action, _context: &mut ComponentContext<'_, ()>) {}

        fn view(&self, _theme: &Theme) -> View<Self::Action> {
            let items = self
                .commands
                .iter()
                .flat_map(|label| {
                    [
                        ToolbarItem::Command {
                            id: (*label).into(),
                            label: (*label).into(),
                            action: *label,
                            enabled: true,
                            variant: ButtonVariant::Standard,
                        },
                        ToolbarItem::Separator,
                    ]
                })
                .collect::<Vec<_>>();
            toolbar(&items)
        }
    }

    #[test]
    fn toolbar_commands_keep_their_retained_identity_across_an_insertion() {
        let mut host = ComponentHost::new(
            Toolbar {
                commands: vec!["Save", "Close"],
            },
            LogicalSize::new(480.0, 120.0),
            Theme::dark(),
        )
        .unwrap();
        let save = node_labeled(&host, "Save");
        host.ui_mut().set_focus(Some(save)).unwrap();
        host.refresh().unwrap();

        host.component_mut().commands.insert(0, "Open");
        host.refresh().unwrap();

        let focused = focused(&host);
        assert_eq!(focused.id, save);
        assert_eq!(focused.data.label, "Save");
    }
}
