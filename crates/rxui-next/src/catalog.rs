//! Component-native composite controls and application surfaces.

use std::{fmt::Display, str::FromStr};

use astrelis_core::geometry::LogicalSize;

use crate::{
    ButtonStyle, ButtonVariant, ColorRole, ContainerStyle, Space, StackStyle, View, button,
    button_with, column, column_with, label, panel, row, row_with, spacer, stack_with, text_field,
    views,
};

/// One controlled radio-group option.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice<Value> {
    /// Domain value emitted when selected.
    pub value: Value,
    /// User-visible label.
    pub label: String,
    /// Whether the choice accepts interaction.
    pub enabled: bool,
}

impl<Value> Choice<Value> {
    /// Creates an enabled choice.
    pub fn new(value: Value, label: impl Into<String>) -> Self {
        Self {
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
    column(views(choices.iter().enumerate().map(|(index, choice)| {
        let marker = if selected == Some(&choice.value) {
            "●"
        } else {
            "○"
        };
        let value = choice.value.clone();
        let on_selected = on_selected.clone();
        button(format!("{marker} {}", choice.label), on_selected(value))
            .enabled(choice.enabled)
            .key(index as u64)
    })))
}

/// One controlled combo-box option.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComboOption<Value> {
    /// Domain value.
    pub value: Value,
    /// User-visible label.
    pub label: String,
    /// Whether the option accepts selection.
    pub enabled: bool,
}

impl<Value> ComboOption<Value> {
    /// Creates an enabled option.
    pub fn new(value: Value, label: impl Into<String>) -> Self {
        Self {
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
            views(options.iter().enumerate().map(|(index, option)| {
                let value = option.value.clone();
                let on_selected = on_selected.clone();
                button(option.label.clone(), on_selected(value))
                    .enabled(option.enabled)
                    .key(index as u64)
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
        /// User-visible label.
        label: String,
        /// Typed action.
        action: Action,
        /// Whether the command accepts interaction.
        enabled: bool,
        /// Semantic presentation.
        variant: ButtonVariant,
    },
    /// Visual separator.
    Separator,
    /// Fixed logical spacing.
    Space(f32),
}

/// Builds a keyed command toolbar.
pub fn toolbar<Action: Clone + 'static>(items: &[ToolbarItem<Action>]) -> View<Action> {
    row_with(
        ContainerStyle::new()
            .gap(Space::Xs)
            .padding(Space::Xs)
            .background(ColorRole::Surface),
        views(items.iter().enumerate().map(|(index, item)| {
            let view = match item {
                ToolbarItem::Command {
                    label,
                    action,
                    enabled,
                    variant,
                } => button_with(
                    label.clone(),
                    action.clone(),
                    ButtonStyle::standard().variant(*variant),
                )
                .enabled(*enabled),
                ToolbarItem::Separator => {
                    panel(LogicalSize::new(1.0, 24.0), ColorRole::Muted, None)
                }
                ToolbarItem::Space(width) => spacer(LogicalSize::new((*width).max(0.0), 1.0)),
            };
            view.key(index as u64)
        })),
    )
}

/// One modal-dialog action.
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
            row(views(actions.iter().enumerate().map(|(index, action)| {
                button_with(
                    action.label.clone(),
                    action.action.clone(),
                    ButtonStyle::standard().variant(action.variant),
                )
                .key(index as u64)
            }))),
        ),
    )
    .visible(open);
    stack_with(
        StackStyle::new().background(ColorRole::Background),
        (background.enabled(!open), modal),
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

/// Builds a controlled keyboard-first command palette surface.
pub fn command_palette<Action: Clone + 'static>(
    query: &str,
    commands: &[CommandItem<Action>],
    selected: usize,
    on_query: impl Fn(String) -> Action + 'static,
) -> View<Action> {
    let query_lower = query.trim().to_lowercase();
    let matches = commands
        .iter()
        .filter(|command| {
            query_lower.is_empty()
                || command.label.to_lowercase().contains(&query_lower)
                || command
                    .description
                    .as_deref()
                    .is_some_and(|value| value.to_lowercase().contains(&query_lower))
        })
        .take(12)
        .collect::<Vec<_>>();
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
}
