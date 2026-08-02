//! Foundational controlled input elements.

use std::{any::Any, ops::RangeInclusive};

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
};
use astrelis_paint::{Brush, Painter};
use astrelis_platform::{CursorIcon, ElementState, Key, NamedKey};
use astrelis_text::{TextLayout, TextLayoutRequest, TextStyle, TextWrap};

use crate::{
    Constraints, Element, EventResult, Invalidation, LayoutContext, SemanticAction,
    SemanticActionKind, SemanticData, SemanticRole, ShapingMemo, UiInput,
};

type BoolAction = dyn Fn(bool) -> Box<dyn Any>;
type ValueAction = dyn Fn(f32) -> Box<dyn Any>;

fn hover_color(color: Color) -> Color {
    Color::new(
        color.r + (1.0 - color.r) * 0.14,
        color.g + (1.0 - color.g) * 0.14,
        color.b + (1.0 - color.b) * 0.14,
        color.a,
    )
}

/// Controlled boolean checkbox with a shaped label.
pub struct Checkbox {
    /// Accessible and visible label.
    pub label: String,
    /// Controlled checked state.
    pub checked: bool,
    /// Preferred size.
    pub size: LogicalSize,
    /// Text color.
    pub text_color: Color,
    /// Outline color.
    pub outline_color: Color,
    /// Checked fill color.
    pub accent_color: Color,
    pressed: bool,
    hovered: bool,
    focused: bool,
    layout: Option<TextLayout>,
    shaped: ShapingMemo,
    changed: Box<BoolAction>,
}

impl Checkbox {
    /// Creates a controlled checkbox.
    pub fn new(
        label: impl Into<String>,
        checked: bool,
        changed: impl Fn(bool) -> Box<dyn Any> + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            checked,
            size: LogicalSize::new(180.0, 28.0),
            text_color: Color::WHITE,
            outline_color: Color::new(0.55, 0.58, 0.64, 1.0),
            accent_color: Color::BLUE,
            pressed: false,
            hovered: false,
            focused: false,
            layout: None,
            shaped: ShapingMemo::default(),
            changed: Box::new(changed),
        }
    }

    /// Replaces the erased change-action factory.
    pub fn set_changed(&mut self, changed: impl Fn(bool) -> Box<dyn Any> + 'static) {
        self.changed = Box::new(changed);
    }

    fn toggle(&mut self) -> EventResult {
        self.checked = !self.checked;
        EventResult {
            action: Some((self.changed)(self.checked)),
            invalidation: Invalidation::PAINT | Invalidation::ACCESSIBILITY,
            clipboard: None,
            handled: true,
        }
    }
}

impl Element for Checkbox {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let mut request = TextLayoutRequest::new(self.label.clone());
        request.style = TextStyle {
            size: 14.0,
            color: self.text_color,
            ..TextStyle::default()
        };
        request.paragraph.wrap = TextWrap::NoWrap;
        self.layout = Some(self.shaped.shape(context, request));
        constraints.constrain(self.size)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        let side = 16.0;
        let indicator = LogicalRect::from_xywh(2.0, (size.height - side) * 0.5, side, side);
        painter.fill_rect(
            indicator,
            Brush::Solid(if self.hovered {
                hover_color(if self.checked {
                    self.accent_color
                } else {
                    self.outline_color
                })
            } else if self.checked {
                self.accent_color
            } else {
                self.outline_color
            }),
        )?;
        if self.checked {
            painter.fill_rect(
                LogicalRect::from_xywh(
                    indicator.origin.x + 4.0,
                    indicator.origin.y + 4.0,
                    side - 8.0,
                    side - 8.0,
                ),
                Brush::Solid(Color::WHITE),
            )?;
        }
        if self.focused {
            painter.fill_rect(
                LogicalRect::from_xywh(0.0, size.height - 2.0, size.width, 2.0),
                Brush::Solid(self.accent_color),
            )?;
        }
        if let Some(layout) = &self.layout {
            painter.draw_text(
                layout,
                LogicalPoint::new(26.0, (size.height - layout.size().height).max(0.0) * 0.5),
                1.0,
            )?;
        }
        Ok(())
    }

    fn accessibility(&self) -> Option<SemanticData> {
        Some(SemanticData {
            role: SemanticRole::Checkbox,
            label: self.label.clone(),
            value: Some(if self.checked { "checked" } else { "unchecked" }.into()),
            ..SemanticData::default()
        })
    }

    fn event(&mut self, input: UiInput) -> EventResult {
        match input {
            UiInput::HoverChanged(hovered) => {
                self.hovered = hovered;
                EventResult {
                    invalidation: Invalidation::PAINT,
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::FocusChanged(focused) => {
                self.focused = focused;
                EventResult {
                    invalidation: Invalidation::PAINT | Invalidation::ACCESSIBILITY,
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::PointerPressed(_) => {
                self.pressed = true;
                EventResult {
                    invalidation: Invalidation::PAINT,
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::PointerReleased(_) if self.pressed => {
                self.pressed = false;
                self.toggle()
            }
            UiInput::Keyboard { input, .. }
                if input.state == ElementState::Pressed
                    && matches!(
                        input.logical_key,
                        Key::Named(NamedKey::Enter | NamedKey::Space)
                    ) =>
            {
                self.toggle()
            }
            _ => EventResult::default(),
        }
    }

    fn semantic_actions(&self) -> Vec<SemanticActionKind> {
        vec![SemanticActionKind::Focus, SemanticActionKind::Activate]
    }

    fn semantic_action(&mut self, action: SemanticAction) -> EventResult {
        match action {
            SemanticAction::Activate => self.toggle(),
            _ => EventResult::default(),
        }
    }

    fn hit_testable(&self) -> bool {
        true
    }
    fn focusable(&self) -> bool {
        true
    }
    fn cursor_icon(&self) -> CursorIcon {
        CursorIcon::Pointer
    }
}

/// Controlled horizontal numeric slider.
pub struct Slider {
    /// Accessible label.
    pub label: String,
    /// Controlled value.
    pub value: f32,
    /// Inclusive accepted range.
    pub range: RangeInclusive<f32>,
    /// Keyboard adjustment step.
    pub step: f32,
    /// Preferred size.
    pub size: LogicalSize,
    /// Track color.
    pub track_color: Color,
    /// Filled track and thumb color.
    pub accent_color: Color,
    dragging: bool,
    hovered: bool,
    focused: bool,
    changed: Box<ValueAction>,
}

impl Slider {
    /// Creates a controlled slider.
    pub fn new(
        label: impl Into<String>,
        value: f32,
        range: RangeInclusive<f32>,
        changed: impl Fn(f32) -> Box<dyn Any> + 'static,
    ) -> Self {
        let start = (*range.start()).min(*range.end());
        let end = (*range.start()).max(*range.end());
        let range = start..=end;
        let value = value.clamp(*range.start(), *range.end());
        Self {
            label: label.into(),
            value,
            range,
            step: 1.0,
            size: LogicalSize::new(180.0, 28.0),
            track_color: Color::new(0.28, 0.3, 0.34, 1.0),
            accent_color: Color::BLUE,
            dragging: false,
            hovered: false,
            focused: false,
            changed: Box::new(changed),
        }
    }

    /// Replaces the erased change-action factory.
    pub fn set_changed(&mut self, changed: impl Fn(f32) -> Box<dyn Any> + 'static) {
        self.changed = Box::new(changed);
    }

    fn fraction(&self) -> f32 {
        let span = *self.range.end() - *self.range.start();
        if span <= f32::EPSILON {
            0.0
        } else {
            (self.value - *self.range.start()) / span
        }
    }

    fn set_from_x(&mut self, x: f32, width: f32) -> EventResult {
        let fraction = (x / width.max(1.0)).clamp(0.0, 1.0);
        self.value = *self.range.start() + fraction * (*self.range.end() - *self.range.start());
        EventResult {
            action: Some((self.changed)(self.value)),
            invalidation: Invalidation::PAINT | Invalidation::ACCESSIBILITY,
            clipboard: None,
            handled: true,
        }
    }

    fn adjust(&mut self, delta: f32) -> EventResult {
        self.value = (self.value + delta).clamp(*self.range.start(), *self.range.end());
        EventResult {
            action: Some((self.changed)(self.value)),
            invalidation: Invalidation::PAINT | Invalidation::ACCESSIBILITY,
            clipboard: None,
            handled: true,
        }
    }
}

impl Element for Slider {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn layout(
        &mut self,
        _context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LogicalSize {
        constraints.constrain(self.size)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        let center = size.height * 0.5;
        let thumb_x = self.fraction() * size.width;
        painter.fill_rect(
            LogicalRect::from_xywh(0.0, center - 2.0, size.width, 4.0),
            Brush::Solid(self.track_color),
        )?;
        painter.fill_rect(
            LogicalRect::from_xywh(0.0, center - 2.0, thumb_x, 4.0),
            Brush::Solid(self.accent_color),
        )?;
        let thumb_height = if self.hovered || self.dragging {
            20.0
        } else {
            16.0
        };
        painter.fill_rect(
            LogicalRect::from_xywh(
                (thumb_x - 6.0).clamp(0.0, (size.width - 12.0).max(0.0)),
                center - thumb_height * 0.5,
                12.0,
                thumb_height,
            ),
            Brush::Solid(if self.focused {
                Color::WHITE
            } else if self.hovered || self.dragging {
                hover_color(self.accent_color)
            } else {
                self.accent_color
            }),
        )?;
        Ok(())
    }

    fn accessibility(&self) -> Option<SemanticData> {
        Some(SemanticData {
            role: SemanticRole::Slider,
            label: self.label.clone(),
            value: Some(self.value.to_string()),
            ..SemanticData::default()
        })
    }

    fn event(&mut self, input: UiInput) -> EventResult {
        match input {
            UiInput::HoverChanged(hovered) => {
                self.hovered = hovered;
                EventResult {
                    invalidation: Invalidation::PAINT,
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::FocusChanged(focused) => {
                self.focused = focused;
                EventResult {
                    invalidation: Invalidation::PAINT | Invalidation::ACCESSIBILITY,
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::PointerPressed(point) => {
                self.dragging = true;
                self.set_from_x(point.x, self.size.width)
            }
            UiInput::PointerMoved(point) if self.dragging => {
                self.set_from_x(point.x, self.size.width)
            }
            UiInput::PointerReleased(point) if self.dragging => {
                self.dragging = false;
                self.set_from_x(point.x, self.size.width)
            }
            UiInput::Keyboard { input, .. } if input.state == ElementState::Pressed => {
                match input.logical_key {
                    Key::Named(NamedKey::Other(ref key)) if key == "ArrowLeft" => {
                        self.adjust(-self.step)
                    }
                    Key::Named(NamedKey::Other(ref key)) if key == "ArrowRight" => {
                        self.adjust(self.step)
                    }
                    _ => EventResult::default(),
                }
            }
            _ => EventResult::default(),
        }
    }

    fn semantic_actions(&self) -> Vec<SemanticActionKind> {
        vec![SemanticActionKind::Focus, SemanticActionKind::SetValue]
    }
    fn semantic_action(&mut self, action: SemanticAction) -> EventResult {
        match action {
            SemanticAction::SetValue(value) => {
                self.value = value.clamp(*self.range.start(), *self.range.end());
                EventResult {
                    action: Some((self.changed)(self.value)),
                    invalidation: Invalidation::PAINT | Invalidation::ACCESSIBILITY,
                    clipboard: None,
                    handled: true,
                }
            }
            _ => EventResult::default(),
        }
    }
    fn hit_testable(&self) -> bool {
        true
    }
    fn focusable(&self) -> bool {
        true
    }
    fn cursor_icon(&self) -> CursorIcon {
        CursorIcon::Pointer
    }
}
