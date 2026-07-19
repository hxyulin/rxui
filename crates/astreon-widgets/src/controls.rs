//! Editor-oriented retained control compositions.

use std::{cell::Cell, fmt, rc::Rc};

use astrelis_core::geometry::{LogicalRect, LogicalSize, Size};
use astrelis_paint::{Brush, Painter, StrokeStyle};
use astrelis_platform::{CursorIcon, ElementState, Key, NamedKey, PointerButton};
use astrelis_ui::widget_any;
use astrelis_ui_core::{
    Button, Column, Edges, ElementHandle, EventContext, EventFilter, Insets, LayoutStyle, Length,
    MountContext, Positioning, RoutedEvent, RoutedEventKind, SemanticAction, SemanticActionKind,
    SemanticRole, TextField, Theme, Ui, UiError, Widget, WidgetContainerStyle, WidgetStyle,
};
use astrelis_ui_widgets::{Menu as PopupMenu, MenuItem as PopupMenuItem};

use crate::icon::{IconView, icons};

/// Diameter of the painted radio indicator circle.
const RADIO_DIAMETER: f32 = 14.0;
/// Diameter of the selected-state accent dot inside the indicator.
const RADIO_DOT: f32 = 6.0;
/// Room reserved around the circle so its centered hairline stroke and the
/// focus ring stay inside the widget bounds instead of being shaved by an
/// ancestor clip (e.g. a scroll view whose edge the group sits on).
const RADIO_RING_MARGIN: f32 = 3.0;

/// One radio-group choice.
pub struct RadioOption {
    /// User-visible label.
    pub label: String,
    /// Whether this choice accepts selection.
    pub enabled: bool,
}

impl RadioOption {
    /// Creates an enabled option.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
        }
    }
}

/// Painted radio indicator owning its visible label, activated through a
/// group-provided callback so the group can track selection state.
struct RadioButton<Message> {
    label: String,
    selected: bool,
    enabled: bool,
    hovered: bool,
    pressed: bool,
    focused: bool,
    on_activate: Rc<dyn Fn(&mut EventContext<'_, Message>)>,
}

impl<Message: 'static> Widget<Message> for RadioButton<Message> {
    widget_any!();

    fn mounted(&mut self, context: &mut MountContext<'_, Message>) -> Result<(), UiError> {
        context.add_label(self.label.clone())?;
        Ok(())
    }

    fn intrinsic_size(&self, theme: &Theme) -> LogicalSize {
        Size::new(RADIO_RING_MARGIN + RADIO_DIAMETER + theme.spacing.sm, 20.0)
    }

    fn container_style(&self, theme: &Theme) -> WidgetContainerStyle {
        WidgetContainerStyle {
            padding: Insets {
                left: RADIO_RING_MARGIN + RADIO_DIAMETER + theme.spacing.sm,
                top: theme.spacing.xs,
                right: theme.spacing.xs,
                bottom: theme.spacing.xs,
            },
            gap: 0.0,
        }
    }

    fn event(&mut self, context: &mut EventContext<'_, Message>, event: &RoutedEvent) {
        if let RoutedEventKind::FocusChanged(focused) = &event.kind {
            self.focused = *focused;
            context.request_paint();
            return;
        }
        if !self.enabled {
            return;
        }
        match &event.kind {
            RoutedEventKind::PointerEntered { .. } => {
                self.hovered = true;
                context.request_paint();
            }
            RoutedEventKind::PointerLeft { .. } | RoutedEventKind::PointerCancelled { .. } => {
                self.hovered = false;
                self.pressed = false;
                context.request_paint();
            }
            RoutedEventKind::PointerButton {
                button: PointerButton::Primary,
                state,
                ..
            } => match state {
                ElementState::Pressed => {
                    self.pressed = true;
                    context.request_focus();
                    context.request_paint();
                }
                ElementState::Released if self.pressed => {
                    self.pressed = false;
                    (self.on_activate.clone())(context);
                    context.request_paint();
                }
                ElementState::Released => {}
            },
            RoutedEventKind::Keyboard(input)
                if input.state == ElementState::Pressed
                    && matches!(
                        input.logical_key,
                        Key::Named(NamedKey::Enter | NamedKey::Space)
                    ) =>
            {
                (self.on_activate.clone())(context);
                context.prevent_default();
            }
            _ => {}
        }
    }

    fn hit_testable(&self) -> bool {
        self.enabled
    }

    fn focusable(&self) -> bool {
        self.enabled
    }

    fn cursor_icon(&self) -> Option<CursorIcon> {
        self.enabled.then_some(CursorIcon::Pointer)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        bounds: LogicalRect,
        theme: &Theme,
    ) -> Result<(), UiError> {
        let circle = LogicalRect::from_xywh(
            bounds.origin.x + RADIO_RING_MARGIN,
            bounds.origin.y + (bounds.size.height - RADIO_DIAMETER) * 0.5,
            RADIO_DIAMETER,
            RADIO_DIAMETER,
        );
        let outline = if !self.enabled {
            theme.disabled_foreground
        } else if self.selected {
            theme.accent
        } else if self.hovered {
            theme.foreground
        } else {
            theme.muted_foreground
        };
        painter
            .stroke_ellipse(
                circle,
                StrokeStyle {
                    width: theme.border_width,
                    ..Default::default()
                },
                Brush::Solid(outline),
            )
            .map_err(control_error)?;
        if self.selected {
            let dot = LogicalRect::from_xywh(
                circle.origin.x + (RADIO_DIAMETER - RADIO_DOT) * 0.5,
                circle.origin.y + (RADIO_DIAMETER - RADIO_DOT) * 0.5,
                RADIO_DOT,
                RADIO_DOT,
            );
            let fill = if self.enabled {
                theme.accent
            } else {
                theme.disabled_foreground
            };
            painter
                .fill_ellipse(dot, Brush::Solid(fill))
                .map_err(control_error)?;
        }
        if self.focused {
            let ring = LogicalRect::from_xywh(
                circle.origin.x - 2.0,
                circle.origin.y - 2.0,
                RADIO_DIAMETER + 4.0,
                RADIO_DIAMETER + 4.0,
            );
            painter
                .stroke_ellipse(
                    ring,
                    StrokeStyle {
                        width: theme.metrics.focus_ring,
                        ..Default::default()
                    },
                    Brush::Solid(theme.accent),
                )
                .map_err(control_error)?;
        }
        Ok(())
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        Some((
            SemanticRole::ListItem,
            self.label.clone(),
            self.selected.then(|| "selected".into()),
        ))
    }

    fn semantic_actions(&self) -> Vec<SemanticActionKind> {
        if self.enabled {
            vec![SemanticActionKind::Focus, SemanticActionKind::Activate]
        } else {
            Vec::new()
        }
    }

    fn semantic_action(
        &mut self,
        context: &mut EventContext<'_, Message>,
        action: &SemanticAction,
    ) -> bool {
        if !self.enabled {
            return false;
        }
        match action {
            SemanticAction::Focus => {
                context.request_focus();
                true
            }
            SemanticAction::Activate => {
                (self.on_activate.clone())(context);
                true
            }
            _ => false,
        }
    }
}

fn control_error(error: impl fmt::Display) -> UiError {
    UiError::from_message(error.to_string())
}

/// Retained single-selection radio group.
pub struct RadioGroup<Message> {
    radios: Vec<ElementHandle<RadioButton<Message>>>,
    enabled: Vec<bool>,
    selected: Rc<Cell<Option<usize>>>,
    _message: std::marker::PhantomData<Message>,
}

impl<Message: 'static> RadioGroup<Message> {
    /// Creates a group and emits `on_select(index)` when selection changes.
    pub fn new<T, F>(
        ui: &mut Ui<Message>,
        parent: ElementHandle<T>,
        options: Vec<RadioOption>,
        selected: Option<usize>,
        on_select: F,
    ) -> Result<Self, UiError>
    where
        F: Fn(usize) -> Message + 'static,
    {
        if selected.is_some_and(|index| index >= options.len()) {
            return Err(UiError::from_message("radio selection is out of range"));
        }
        let group = ui.add_column(parent)?;
        ui.set_semantic_role(group, SemanticRole::List)?;
        let state = Rc::new(Cell::new(selected));
        let on_select: Rc<dyn Fn(usize) -> Message> = Rc::new(on_select);
        let enabled = options
            .iter()
            .map(|option| option.enabled)
            .collect::<Vec<_>>();
        let mut radios = Vec::with_capacity(options.len());
        for (index, option) in options.into_iter().enumerate() {
            let state_for_click = state.clone();
            let message_for_click = on_select.clone();
            let radio = ui.add_widget(
                group,
                RadioButton {
                    label: option.label,
                    selected: selected == Some(index),
                    enabled: option.enabled,
                    hovered: false,
                    pressed: false,
                    focused: false,
                    on_activate: Rc::new(move |context| {
                        state_for_click.set(Some(index));
                        context.emit(message_for_click(index));
                    }),
                },
            )?;
            ui.set_enabled(radio, option.enabled)?;
            radios.push(radio);
        }
        for (index, radio) in radios.iter().copied().enumerate() {
            let all_enabled = enabled.clone();
            let all_radios = radios.clone();
            let state_for_key = state.clone();
            let message_for_key = on_select.clone();
            ui.listen(
                radio,
                None,
                EventFilter::Keyboard,
                move |context, event| {
                    let RoutedEventKind::Keyboard(input) = &event.kind else {
                        return;
                    };
                    if input.state != ElementState::Pressed {
                        return;
                    }
                    let Key::Named(key) = &input.logical_key else {
                        return;
                    };
                    let direction = match key {
                        NamedKey::Other(value) if value == "ArrowDown" || value == "ArrowRight" => {
                            1
                        }
                        NamedKey::Other(value) if value == "ArrowUp" || value == "ArrowLeft" => -1,
                        NamedKey::Other(value) if value == "Home" => i32::MIN,
                        NamedKey::Other(value) if value == "End" => i32::MAX,
                        _ => return,
                    };
                    let next = next_enabled(index, direction, &all_enabled);
                    if let Some(next) = next {
                        state_for_key.set(Some(next));
                        context.emit(message_for_key(next));
                        if let Some(handle) = all_radios.get(next).copied() {
                            context.request_focus_for(handle);
                        }
                        context.prevent_default();
                    }
                },
            )?;
        }
        Ok(Self {
            radios,
            enabled,
            selected: state,
            _message: std::marker::PhantomData,
        })
    }

    /// Returns the selected option.
    pub fn selected(&self) -> Option<usize> {
        self.selected.get()
    }

    /// Updates the selected option and visible markers.
    pub fn set_selected(
        &self,
        ui: &mut Ui<Message>,
        selected: Option<usize>,
    ) -> Result<(), UiError> {
        if selected.is_some_and(|index| index >= self.radios.len() || !self.enabled[index]) {
            return Err(UiError::from_message(
                "radio selection is invalid or disabled",
            ));
        }
        self.selected.set(selected);
        for (index, radio) in self.radios.iter().copied().enumerate() {
            ui.update_widget(radio, |radio| radio.selected = selected == Some(index))?;
        }
        Ok(())
    }
}

fn next_enabled(current: usize, direction: i32, enabled: &[bool]) -> Option<usize> {
    if enabled.is_empty() || !enabled.iter().any(|value| *value) {
        return None;
    }
    if direction == i32::MIN {
        return enabled.iter().position(|value| *value);
    }
    if direction == i32::MAX {
        return enabled.iter().rposition(|value| *value);
    }
    let mut index = current;
    for _ in 0..enabled.len() {
        index = if direction > 0 {
            (index + 1) % enabled.len()
        } else {
            (index + enabled.len() - 1) % enabled.len()
        };
        if enabled[index] {
            return Some(index);
        }
    }
    None
}

/// One combo-box choice and its typed message.
pub struct ComboBoxItem<Message> {
    /// User-visible label.
    pub label: String,
    /// Message emitted on selection.
    pub message: Message,
    /// Whether the item accepts selection.
    pub enabled: bool,
}

/// Retained button and popup implementing a single-selection combo box.
pub struct ComboBox<Message> {
    owner: ElementHandle<Button>,
    labels: Vec<String>,
    selected: Option<usize>,
    /// Popup controller used for open/close and focus inspection.
    pub popup: PopupMenu,
    _message: std::marker::PhantomData<Message>,
}

impl<Message: Clone + 'static> ComboBox<Message> {
    /// Creates a combo box from typed entries.
    pub fn new<T>(
        ui: &mut Ui<Message>,
        parent: ElementHandle<T>,
        placeholder: impl Into<String>,
        items: Vec<ComboBoxItem<Message>>,
        selected: Option<usize>,
    ) -> Result<Self, UiError> {
        if selected.is_some_and(|index| index >= items.len()) {
            return Err(UiError::from_message("combo-box selection is out of range"));
        }
        let labels = items
            .iter()
            .map(|item| item.label.clone())
            .collect::<Vec<_>>();
        let owner_label = selected
            .and_then(|index| labels.get(index).cloned())
            .unwrap_or_else(|| placeholder.into());
        let owner = ui.add_button(parent, owner_label)?;
        ui.set_layout(
            owner,
            LayoutStyle {
                // Fixed rather than min width, so the owner keeps its size
                // instead of stretching to fill an aligned column.
                width: Length::Px(180.0),
                ..Default::default()
            },
        )?;
        let indicator = ui.add_widget(owner, IconView::new(icons::chevron_down(), 14.0))?;
        ui.set_layout(
            indicator,
            LayoutStyle {
                width: Length::Px(14.0),
                height: Length::Px(14.0),
                positioning: Positioning::Absolute,
                inset: Edges {
                    left: Length::Auto,
                    top: Length::Px(7.0),
                    right: Length::Px(10.0),
                    bottom: Length::Auto,
                },
                ..Default::default()
            },
        )?;
        let popup = PopupMenu::new(
            ui,
            owner,
            items
                .into_iter()
                .map(|item| PopupMenuItem {
                    label: item.label,
                    message: item.message,
                    enabled: item.enabled,
                })
                .collect(),
        )?;
        Ok(Self {
            owner,
            labels,
            selected,
            popup,
            _message: std::marker::PhantomData,
        })
    }

    /// Returns the current selected index.
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Updates the selected value shown by the owner button.
    pub fn set_selected(
        &mut self,
        ui: &mut Ui<Message>,
        selected: Option<usize>,
    ) -> Result<(), UiError> {
        if selected.is_some_and(|index| index >= self.labels.len()) {
            return Err(UiError::from_message("combo-box selection is out of range"));
        }
        self.selected = selected;
        let label = selected
            .and_then(|index| self.labels.get(index))
            .map(String::as_str)
            .unwrap_or("Select…");
        ui.set_button_text(self.owner, label)
    }
}

/// Numeric-field bounds and stepping policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumericFieldOptions {
    /// Inclusive minimum.
    pub min: f64,
    /// Inclusive maximum.
    pub max: f64,
    /// Positive increment.
    pub step: f64,
    /// Decimal places used for display.
    pub decimals: usize,
}

impl Default for NumericFieldOptions {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 100.0,
            step: 1.0,
            decimals: 0,
        }
    }
}

/// Text field with increment and decrement buttons.
pub struct NumericField<Message> {
    /// Editable text handle.
    pub field: ElementHandle<TextField>,
    /// Decrement button.
    pub decrement: ElementHandle<Button>,
    /// Increment button.
    pub increment: ElementHandle<Button>,
    value: Rc<Cell<f64>>,
    options: NumericFieldOptions,
    _message: std::marker::PhantomData<Message>,
}

impl<Message: 'static> NumericField<Message> {
    /// Creates a numeric field and emits normalized values.
    pub fn new<T, F>(
        ui: &mut Ui<Message>,
        parent: ElementHandle<T>,
        value: f64,
        options: NumericFieldOptions,
        on_change: F,
    ) -> Result<Self, UiError>
    where
        F: Fn(f64) -> Message + 'static,
    {
        validate_numeric(options)?;
        let value = normalize(value, options);
        let row = ui.add_row(parent)?;
        let field = ui.add_text_field(row, format_value(value, options.decimals))?;
        ui.set_layout(
            field,
            LayoutStyle {
                width: Length::Px(96.0),
                ..Default::default()
            },
        )?;
        let decrement = ui.add_button(row, "−")?;
        let increment = ui.add_button(row, "+")?;
        let state = Rc::new(Cell::new(value));
        let callback: Rc<dyn Fn(f64) -> Message> = Rc::new(on_change);
        for (button, direction) in [(decrement, -1.0), (increment, 1.0)] {
            let state = state.clone();
            let callback = callback.clone();
            ui.listen(button, None, EventFilter::Activate, move |context, _| {
                let value = normalize(state.get() + options.step * direction, options);
                state.set(value);
                context.emit(callback(value));
            })?;
        }
        let state_for_text = state.clone();
        let callback_for_text = callback.clone();
        ui.listen(
            field,
            None,
            EventFilter::ValueChanged,
            move |context, event| {
                let RoutedEventKind::TextSubmitted(text) = &event.kind else {
                    return;
                };
                let value = text
                    .parse::<f64>()
                    .ok()
                    .map(|value| normalize(value, options))
                    .unwrap_or_else(|| state_for_text.get());
                state_for_text.set(value);
                context.emit(callback_for_text(value));
            },
        )?;
        let state_for_key = state.clone();
        let callback_for_key = callback.clone();
        ui.listen(field, None, EventFilter::Keyboard, move |context, event| {
            let RoutedEventKind::Keyboard(input) = &event.kind else {
                return;
            };
            if input.state != ElementState::Pressed {
                return;
            }
            let direction = match &input.logical_key {
                Key::Named(NamedKey::Other(key)) if key == "ArrowUp" => 1.0,
                Key::Named(NamedKey::Other(key)) if key == "ArrowDown" => -1.0,
                _ => return,
            };
            let value = normalize(state_for_key.get() + options.step * direction, options);
            state_for_key.set(value);
            context.emit(callback_for_key(value));
            context.prevent_default();
        })?;
        Ok(Self {
            field,
            decrement,
            increment,
            value: state,
            options,
            _message: std::marker::PhantomData,
        })
    }

    /// Returns the last normalized value.
    pub fn value(&self) -> f64 {
        self.value.get()
    }

    /// Updates retained numeric state and formatted text.
    pub fn set_value(&self, ui: &mut Ui<Message>, value: f64) -> Result<(), UiError> {
        let value = normalize(value, self.options);
        self.value.set(value);
        ui.set_text(self.field, format_value(value, self.options.decimals))
    }
}

fn validate_numeric(options: NumericFieldOptions) -> Result<(), UiError> {
    if !options.min.is_finite()
        || !options.max.is_finite()
        || !options.step.is_finite()
        || options.min >= options.max
        || options.step <= 0.0
    {
        return Err(UiError::from_message(
            "numeric fields require finite min < max and a positive step",
        ));
    }
    Ok(())
}

fn normalize(value: f64, options: NumericFieldOptions) -> f64 {
    let value = if value.is_finite() {
        value
    } else {
        options.min
    };
    let steps = ((value - options.min) / options.step).round();
    (options.min + steps * options.step).clamp(options.min, options.max)
}

fn format_value(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}")
}

/// Titled form grouping with optional explanatory text.
#[derive(Clone, Copy)]
pub struct FormSection {
    /// Column receiving form rows and controls.
    pub content: ElementHandle<Column>,
    /// Section title label.
    pub title: ElementHandle<astrelis_ui_core::Label>,
    /// Optional explanatory label under the title.
    pub help: Option<ElementHandle<astrelis_ui_core::Label>>,
}

impl FormSection {
    /// Creates a titled form section.
    pub fn new<Message: 'static, T>(
        ui: &mut Ui<Message>,
        parent: ElementHandle<T>,
        title: impl Into<String>,
        help: Option<&str>,
    ) -> Result<Self, UiError> {
        let section = ui.add_column(parent)?;
        ui.set_semantic_role(section, SemanticRole::Form)?;
        // Separate consecutive sections without relying on the parent's gap.
        let mut layout = LayoutStyle::default();
        layout.margin.top = Length::Px(ui.theme().spacing.lg);
        ui.set_layout(section, layout)?;
        let title = ui.add_label(section, title)?;
        let help = help
            .map(|help| ui.add_label(section, help))
            .transpose()?;
        let content = ui.add_column(section)?;
        let this = Self {
            content,
            title,
            help,
        };
        this.restyle(ui)?;
        Ok(this)
    }

    /// Re-applies theme-derived title and help styles.
    ///
    /// Widget-style color overrides snapshot the theme they were resolved
    /// against, so call this after `set_theme` to keep the help text tinted
    /// with the current `muted_foreground`.
    pub fn restyle<Message: 'static>(&self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        ui.set_widget_style(
            self.title,
            WidgetStyle {
                font_size: Some(ui.theme().type_scale.heading),
                font_weight: Some(ui.theme().type_scale.heading_weight),
                ..Default::default()
            },
        )?;
        if let Some(help) = self.help {
            ui.set_widget_style(
                help,
                WidgetStyle {
                    foreground: Some(ui.theme().muted_foreground),
                    font_size: Some(ui.theme().type_scale.caption),
                    ..Default::default()
                },
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use astrelis_core::geometry::Size;
    use astrelis_paint::Command;
    use astrelis_text::FontDatabase;
    use astrelis_ui_core::Theme;

    use super::*;

    #[test]
    fn numeric_values_clamp_and_snap() {
        let options = NumericFieldOptions {
            min: -1.0,
            max: 1.0,
            step: 0.25,
            decimals: 2,
        };
        assert_eq!(normalize(0.62, options), 0.5);
        assert_eq!(normalize(9.0, options), 1.0);
        assert_eq!(normalize(f64::NAN, options), -1.0);
    }

    #[test]
    fn radio_navigation_skips_disabled_options() {
        assert_eq!(next_enabled(0, 1, &[true, false, true]), Some(2));
        assert_eq!(next_enabled(2, 1, &[true, false, true]), Some(0));
    }

    #[test]
    fn radio_markers_are_painted_ellipses() {
        let mut ui = Ui::new(FontDatabase::default(), Theme::default());
        let root = ui.root();
        let group = RadioGroup::new(
            &mut ui,
            root,
            vec![RadioOption::new("First"), RadioOption::new("Second")],
            Some(0),
            |index| index,
        )
        .unwrap();
        ui.set_viewport(Size::new(320.0, 240.0), 1.0);
        let display_list = ui.display_list().unwrap();
        let strokes = display_list
            .commands()
            .iter()
            .filter(|command| matches!(command, Command::StrokeEllipse { .. }))
            .count();
        let fills = display_list
            .commands()
            .iter()
            .filter(|command| matches!(command, Command::FillEllipse { .. }))
            .count();
        assert_eq!(strokes, 2, "one outline circle per option");
        assert_eq!(fills, 1, "only the selected option paints a dot");

        group.set_selected(&mut ui, None).unwrap();
        let display_list = ui.display_list().unwrap();
        assert!(
            !display_list
                .commands()
                .iter()
                .any(|command| matches!(command, Command::FillEllipse { .. }))
        );
    }

    #[test]
    fn combo_box_indicator_is_a_vector_path() {
        let mut ui = Ui::new(FontDatabase::default(), Theme::default());
        let root = ui.root();
        ComboBox::new(
            &mut ui,
            root,
            "Select…",
            vec![ComboBoxItem {
                label: "Medium".into(),
                message: (),
                enabled: true,
            }],
            Some(0),
        )
        .unwrap();
        ui.set_viewport(Size::new(320.0, 120.0), 1.0);
        let display_list = ui.display_list().unwrap();
        assert!(
            display_list
                .commands()
                .iter()
                .any(|command| matches!(command, Command::FillPath { .. }))
        );
    }
}
