//! Controlled combo boxes with an inline popup surface.

use std::sync::Arc;

use crate::{
    ColorRole, ContainerStyle, Space, View, ViewKey, button, column, column_with, label, views,
};

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
    label_text: impl Into<Arc<str>>,
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
