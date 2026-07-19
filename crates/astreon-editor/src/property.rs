//! Typed, message-driven property inspector.

use std::rc::Rc;

use astreon_widgets::ui_core::{
    Column, ElementHandle, EventFilter, LayoutStyle, Length, RoutedEventKind, SemanticRole, Ui,
    UiError,
};
use astreon_widgets::{
    ComboBox, ComboBoxItem, IconButton, NumericField, NumericFieldOptions, ValidationResult, icons,
};

/// Value edited by a core property field.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
    /// Single-line UTF-8 text.
    Text(String),
    /// Bounded numeric value.
    Number {
        /// Controlled numeric value.
        value: f64,
        /// Bounds, stepping, and formatting policy.
        options: NumericFieldOptions,
    },
    /// Boolean toggle.
    Boolean(bool),
    /// One selected value from an ordered choice list.
    Choice {
        /// Controlled option index.
        selected: usize,
        /// Ordered user-visible choices.
        options: Vec<String>,
    },
}

/// One labeled property.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyField<Id> {
    /// Stable application identity.
    pub id: Id,
    /// User-visible label.
    pub label: String,
    /// Controlled value and editor kind.
    pub value: PropertyValue,
    /// Optional validation presentation.
    pub validation: ValidationResult,
    /// Whether the editor accepts input.
    pub enabled: bool,
}

/// One collapsible property group.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertySection<Id> {
    /// Stable application identity.
    pub id: Id,
    /// User-visible title.
    pub title: String,
    /// Controlled expansion state.
    pub expanded: bool,
    /// Ordered fields.
    pub fields: Vec<PropertyField<Id>>,
}

/// Interaction emitted by a [`PropertyGrid`].
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyAction<Id> {
    /// Replace one controlled field value.
    Change {
        /// Stable field identity.
        id: Id,
        /// Requested replacement value.
        value: PropertyValue,
    },
    /// Change one section's controlled expansion state.
    SetSectionExpanded {
        /// Stable section identity.
        id: Id,
        /// Requested expansion state.
        expanded: bool,
    },
}

/// Reconciled property inspector using Astreon's form controls.
pub struct PropertyGrid<Id, Message> {
    root: ElementHandle<Column>,
    rendered: Vec<ElementHandle<Column>>,
    map_action: Rc<dyn Fn(PropertyAction<Id>) -> Message>,
}

impl<Id, Message> PropertyGrid<Id, Message>
where
    Id: Clone + 'static,
    Message: Clone + 'static,
{
    /// Creates an empty property grid.
    pub fn new<T>(
        ui: &mut Ui<Message>,
        parent: ElementHandle<T>,
        map_action: impl Fn(PropertyAction<Id>) -> Message + 'static,
    ) -> Result<Self, UiError> {
        let root = ui.add_column(parent)?;
        ui.set_semantic_role(root, SemanticRole::Form)?;
        ui.set_layout(
            root,
            LayoutStyle {
                width: Length::Percent(1.0),
                ..Default::default()
            },
        )?;
        Ok(Self {
            root,
            rendered: Vec::new(),
            map_action: Rc::new(map_action),
        })
    }

    /// Returns the inspector root.
    pub const fn root(&self) -> ElementHandle<Column> {
        self.root
    }

    /// Rebuilds the inspector from controlled sections.
    pub fn sync(
        &mut self,
        ui: &mut Ui<Message>,
        sections: &[PropertySection<Id>],
    ) -> Result<(), UiError> {
        for section in self.rendered.drain(..) {
            ui.remove(section)?;
        }
        for section in sections {
            let container = ui.add_column(self.root)?;
            let id = section.id.clone();
            let expanded = !section.expanded;
            let mapper = self.map_action.clone();
            let toggle = ui.add_widget(
                container,
                IconButton::labelled(
                    if section.expanded {
                        icons::chevron_down()
                    } else {
                        icons::chevron_right()
                    },
                    section.title.clone(),
                    mapper(PropertyAction::SetSectionExpanded { id, expanded }),
                ),
            )?;
            ui.set_layout(
                toggle,
                LayoutStyle {
                    width: Length::Percent(1.0),
                    ..Default::default()
                },
            )?;
            if section.expanded {
                for field in &section.fields {
                    self.build_field(ui, container, field)?;
                }
            }
            self.rendered.push(container);
        }
        Ok(())
    }

    fn build_field(
        &self,
        ui: &mut Ui<Message>,
        parent: ElementHandle<Column>,
        field: &PropertyField<Id>,
    ) -> Result<(), UiError> {
        let row = ui.add_row(parent)?;
        ui.set_layout(
            row,
            LayoutStyle {
                width: Length::Percent(1.0),
                ..Default::default()
            },
        )?;
        let label = ui.add_label(row, &field.label)?;
        ui.set_layout(
            label,
            LayoutStyle {
                width: Length::Px(84.0),
                shrink: 0.0,
                ..Default::default()
            },
        )?;
        let invalid = field.validation.has_errors();
        match &field.value {
            PropertyValue::Text(value) => {
                let editor = ui.add_text_field(row, value)?;
                ui.set_layout(
                    editor,
                    LayoutStyle {
                        grow: 1.0,
                        min_width: Length::Px(48.0),
                        ..Default::default()
                    },
                )?;
                ui.set_enabled(editor, field.enabled)?;
                ui.set_semantic_invalid(editor, invalid)?;
                if let Some(message) = field.validation.message() {
                    ui.set_semantic_description(editor, Some(message.into()))?;
                }
                let id = field.id.clone();
                let mapper = self.map_action.clone();
                ui.listen(
                    editor,
                    None,
                    EventFilter::ValueChanged,
                    move |context, event| {
                        if let RoutedEventKind::TextSubmitted(value) = &event.kind {
                            context.emit(mapper(PropertyAction::Change {
                                id: id.clone(),
                                value: PropertyValue::Text(value.clone()),
                            }));
                        }
                    },
                )?;
            }
            PropertyValue::Number { value, options } => {
                let id = field.id.clone();
                let mapper = self.map_action.clone();
                let options = *options;
                let editor = NumericField::new(ui, row, *value, options, move |value| {
                    mapper(PropertyAction::Change {
                        id: id.clone(),
                        value: PropertyValue::Number { value, options },
                    })
                })?;
                ui.set_layout(
                    editor.root,
                    LayoutStyle {
                        grow: 1.0,
                        min_width: Length::Px(0.0),
                        ..Default::default()
                    },
                )?;
                ui.set_enabled(editor.field, field.enabled)?;
                ui.set_enabled(editor.decrement, field.enabled)?;
                ui.set_enabled(editor.increment, field.enabled)?;
                ui.set_semantic_invalid(editor.field, invalid)?;
                if let Some(message) = field.validation.message() {
                    ui.set_semantic_description(editor.field, Some(message.into()))?;
                }
            }
            PropertyValue::Boolean(value) => {
                let editor = ui.add_checkbox(row, *value)?;
                ui.set_enabled(editor, field.enabled)?;
                let id = field.id.clone();
                let mapper = self.map_action.clone();
                ui.listen(
                    editor,
                    None,
                    EventFilter::ValueChanged,
                    move |context, event| {
                        if let RoutedEventKind::CheckedChanged(value) = event.kind {
                            context.emit(mapper(PropertyAction::Change {
                                id: id.clone(),
                                value: PropertyValue::Boolean(value),
                            }));
                        }
                    },
                )?;
            }
            PropertyValue::Choice { selected, options } => {
                if *selected >= options.len() {
                    return Err(UiError::from_message(
                        "property choice selection is out of range",
                    ));
                }
                let id = field.id.clone();
                let mapper = self.map_action.clone();
                let all_options = options.clone();
                let items = options
                    .iter()
                    .enumerate()
                    .map(|(index, label)| ComboBoxItem {
                        label: label.clone(),
                        message: mapper(PropertyAction::Change {
                            id: id.clone(),
                            value: PropertyValue::Choice {
                                selected: index,
                                options: all_options.clone(),
                            },
                        }),
                        enabled: field.enabled,
                    })
                    .collect();
                let combo = ComboBox::new(ui, row, "Select…", items, Some(*selected))?;
                ui.set_layout(
                    combo.owner(),
                    LayoutStyle {
                        grow: 1.0,
                        min_width: Length::Px(48.0),
                        ..Default::default()
                    },
                )?;
                ui.set_enabled(combo.owner(), field.enabled)?;
            }
        }
        if let Some(message) = field.validation.message() {
            ui.add_label(parent, message)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_property_values_are_owned_and_comparable() {
        assert_eq!(PropertyValue::Boolean(true), PropertyValue::Boolean(true));
        assert_ne!(
            PropertyValue::Text("a".into()),
            PropertyValue::Text("b".into())
        );
    }
}
