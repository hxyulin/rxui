//! Retained editable text control used by the larger validation slice.

use std::any::Any;

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
};
use astrelis_paint::{Brush, Painter};
use astrelis_platform::{CursorIcon, ElementState, ImeEvent, Key, NamedKey};
use astrelis_text::{
    CaretMovement, ParagraphStyle, TextLayout, TextLayoutRequest, TextPosition, TextStyle, TextWrap,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    ClipboardOperation, Constraints, Element, EventResult, Invalidation, LayoutContext,
    SemanticAction, SemanticActionKind, SemanticData, SemanticRole, ShapingMemo, UiInput,
};

type TextActionFactory = dyn Fn(String) -> Box<dyn Any>;

fn hover_color(color: Color) -> Color {
    Color::new(
        color.r + (1.0 - color.r) * 0.08,
        color.g + (1.0 - color.g) * 0.08,
        color.b + (1.0 - color.b) * 0.08,
        color.a,
    )
}

/// Single-line retained text editor with shaped selection and caret geometry.
pub struct TextField {
    /// Accessible name.
    pub label: String,
    /// Controlled text value.
    pub text: String,
    /// Preferred control width.
    pub width: f32,
    /// Font size.
    pub font_size: f32,
    /// Text color.
    pub text_color: Color,
    /// Background color.
    pub background: Color,
    /// Selection color.
    pub selection_color: Color,
    /// Caret color.
    pub caret_color: Color,
    caret: TextPosition,
    anchor: TextPosition,
    preedit: String,
    focused: bool,
    hovered: bool,
    layout: Option<TextLayout>,
    placeholder_layout: Option<TextLayout>,
    shaped: ShapingMemo,
    shaped_placeholder: ShapingMemo,
    changed: Option<Box<TextActionFactory>>,
    submitted: Option<Box<TextActionFactory>>,
}

impl TextField {
    const PADDING_X: f32 = 8.0;
    const PADDING_Y: f32 = 5.0;

    /// Creates an editable field.
    pub fn new(label: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let caret = TextPosition {
            byte_index: text.len(),
            ..TextPosition::default()
        };
        Self {
            label: label.into(),
            text,
            width: 160.0,
            font_size: 14.0,
            text_color: Color::WHITE,
            background: Color::new(0.12, 0.13, 0.15, 1.0),
            selection_color: Color::new(0.12, 0.35, 0.72, 0.7),
            caret_color: Color::WHITE,
            caret,
            anchor: caret,
            preedit: String::new(),
            focused: false,
            hovered: false,
            layout: None,
            placeholder_layout: None,
            shaped: ShapingMemo::default(),
            shaped_placeholder: ShapingMemo::default(),
            changed: None,
            submitted: None,
        }
    }

    /// Emits a typed action whenever editing changes the value.
    pub fn on_changed<A: Any>(mut self, callback: impl Fn(String) -> A + 'static) -> Self {
        self.changed = Some(Box::new(move |text| Box::new(callback(text))));
        self
    }

    /// Installs an erased change-action factory for component runtimes.
    pub fn on_changed_factory(
        mut self,
        callback: impl Fn(String) -> Box<dyn Any> + 'static,
    ) -> Self {
        self.changed = Some(Box::new(callback));
        self
    }

    /// Replaces the erased change-action factory without recreating the field.
    pub fn set_changed_factory(&mut self, callback: impl Fn(String) -> Box<dyn Any> + 'static) {
        self.changed = Some(Box::new(callback));
    }

    /// Emits a typed action when Enter submits the current value.
    pub fn on_submitted<A: Any>(mut self, callback: impl Fn(String) -> A + 'static) -> Self {
        self.submitted = Some(Box::new(move |text| Box::new(callback(text))));
        self
    }

    /// Installs an erased submit-action factory for component runtimes.
    pub fn on_submitted_factory(
        mut self,
        callback: impl Fn(String) -> Box<dyn Any> + 'static,
    ) -> Self {
        self.submitted = Some(Box::new(callback));
        self
    }

    /// Replaces the erased submit-action factory without recreating the field.
    pub fn set_submitted_factory(&mut self, callback: impl Fn(String) -> Box<dyn Any> + 'static) {
        self.submitted = Some(Box::new(callback));
    }

    /// Current selection in normalized UTF-8 byte order.
    pub fn selection(&self) -> (usize, usize) {
        let a = self.anchor.byte_index.min(self.text.len());
        let b = self.caret.byte_index.min(self.text.len());
        (a.min(b), a.max(b))
    }

    /// Replaces the controlled value and clamps retained selection positions.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.caret.byte_index = self.caret.byte_index.min(self.text.len());
        self.anchor.byte_index = self.anchor.byte_index.min(self.text.len());
    }

    fn shown_text(&self) -> String {
        if self.preedit.is_empty() {
            return self.text.clone();
        }
        let mut shown = self.text.clone();
        shown.insert_str(self.caret.byte_index.min(shown.len()), &self.preedit);
        shown
    }

    fn replace_selection(&mut self, value: &str) {
        let (start, end) = self.selection();
        self.text.replace_range(start..end, value);
        self.caret = TextPosition {
            byte_index: start + value.len(),
            ..TextPosition::default()
        };
        self.anchor = self.caret;
    }

    fn previous_grapheme(&self, index: usize) -> Option<usize> {
        self.text[..index.min(self.text.len())]
            .grapheme_indices(true)
            .next_back()
            .map(|(index, _)| index)
    }

    fn next_grapheme(&self, index: usize) -> Option<usize> {
        let index = index.min(self.text.len());
        self.text[index..]
            .grapheme_indices(true)
            .nth(1)
            .map(|(offset, _)| index + offset)
            .or_else(|| (index < self.text.len()).then_some(self.text.len()))
    }

    fn changed_result(&self) -> EventResult {
        EventResult {
            action: self
                .changed
                .as_ref()
                .map(|callback| callback(self.text.clone())),
            invalidation: Invalidation::LAYOUT_ALL,
            clipboard: None,
            handled: true,
        }
    }

    fn move_caret(&mut self, movement: CaretMovement, extend: bool) {
        let Some(layout) = &self.layout else {
            return;
        };
        self.caret = layout.move_caret(self.caret, movement);
        self.caret.byte_index = self.caret.byte_index.min(self.text.len());
        if !extend {
            self.anchor = self.caret;
        }
    }

    fn clamp_boundary(&self, mut index: usize) -> usize {
        index = index.min(self.text.len());
        while !self.text.is_char_boundary(index) {
            index -= 1;
        }
        index
    }
}

impl Element for TextField {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let available = (self.width - Self::PADDING_X * 2.0).max(0.0);
        let mut request = TextLayoutRequest::new(self.shown_text());
        request.style = TextStyle {
            size: self.font_size.max(1.0),
            color: self.text_color,
            ..TextStyle::default()
        };
        request.paragraph = ParagraphStyle {
            max_width: Some(available),
            wrap: TextWrap::NoWrap,
            ..ParagraphStyle::default()
        };
        let layout = self.shaped.shape(context, request);
        let mut placeholder = TextLayoutRequest::new(self.label.clone());
        placeholder.style = TextStyle {
            size: self.font_size.max(1.0),
            color: Color::new(0.58, 0.61, 0.68, 1.0),
            ..TextStyle::default()
        };
        placeholder.paragraph = ParagraphStyle {
            max_width: Some(available),
            wrap: TextWrap::NoWrap,
            ..ParagraphStyle::default()
        };
        let placeholder_layout = self.shaped_placeholder.shape(context, placeholder);
        let desired = LogicalSize::new(
            self.width,
            layout.size().height.max(placeholder_layout.size().height) + Self::PADDING_Y * 2.0,
        );
        self.layout = Some(layout);
        self.placeholder_layout = Some(placeholder_layout);
        constraints.constrain(desired)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        painter.fill_rect(
            LogicalRect::from_xywh(0.0, 0.0, size.width, size.height),
            Brush::Solid(if self.hovered && !self.focused {
                hover_color(self.background)
            } else {
                self.background
            }),
        )?;
        let Some(layout) = &self.layout else {
            return Ok(());
        };
        let origin = LogicalPoint::new(Self::PADDING_X, Self::PADDING_Y);
        let (start, end) = self.selection();
        if start != end {
            for rect in layout.selection_rects(
                TextPosition {
                    byte_index: start,
                    ..TextPosition::default()
                },
                TextPosition {
                    byte_index: end,
                    ..TextPosition::default()
                },
            ) {
                painter.fill_rect(
                    LogicalRect::from_xywh(
                        origin.x + rect.origin.x,
                        origin.y + rect.origin.y,
                        rect.size.width,
                        rect.size.height,
                    ),
                    Brush::Solid(self.selection_color),
                )?;
            }
        }
        painter.draw_text(layout, origin, 1.0)?;
        if self.text.is_empty()
            && self.preedit.is_empty()
            && let Some(placeholder) = &self.placeholder_layout
        {
            painter.draw_text(placeholder, origin, 1.0)?;
        }
        if self.focused {
            let caret = layout.caret_rect(self.caret, 1.0);
            painter.fill_rect(
                LogicalRect::from_xywh(
                    origin.x + caret.origin.x,
                    origin.y + caret.origin.y,
                    caret.size.width.max(1.0),
                    caret.size.height,
                ),
                Brush::Solid(self.caret_color),
            )?;
        }
        Ok(())
    }

    fn accessibility(&self) -> Option<SemanticData> {
        Some(SemanticData {
            role: SemanticRole::TextField,
            label: self.label.clone(),
            value: Some(self.text.clone()),
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
                if let Some(layout) = &self.layout {
                    let point =
                        LogicalPoint::new(point.x - Self::PADDING_X, point.y - Self::PADDING_Y);
                    self.caret = layout.hit_test(point).position;
                    self.caret.byte_index = self.caret.byte_index.min(self.text.len());
                    self.anchor = self.caret;
                }
                EventResult {
                    invalidation: Invalidation::PAINT | Invalidation::ACCESSIBILITY,
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::Keyboard { input, modifiers } if input.state == ElementState::Pressed => {
                let command = modifiers.control || modifiers.super_key;
                if command
                    && matches!(&input.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("a"))
                {
                    self.anchor.byte_index = 0;
                    self.caret.byte_index = self.text.len();
                    return EventResult {
                        invalidation: Invalidation::PAINT | Invalidation::ACCESSIBILITY,
                        handled: true,
                        ..EventResult::default()
                    };
                }
                if command
                    && matches!(&input.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("c"))
                {
                    let (start, end) = self.selection();
                    return EventResult {
                        clipboard: (start != end).then(|| {
                            ClipboardOperation::WriteText(self.text[start..end].to_owned())
                        }),
                        handled: true,
                        ..EventResult::default()
                    };
                }
                if command
                    && matches!(&input.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("x"))
                {
                    let (start, end) = self.selection();
                    if start != end {
                        let selected = self.text[start..end].to_owned();
                        self.replace_selection("");
                        let mut result = self.changed_result();
                        result.clipboard = Some(ClipboardOperation::WriteText(selected));
                        return result;
                    }
                    return EventResult {
                        handled: true,
                        ..EventResult::default()
                    };
                }
                let mut handled = false;
                match &input.logical_key {
                    Key::Named(NamedKey::Backspace) => {
                        handled = true;
                        let (start, end) = self.selection();
                        if start != end {
                            self.replace_selection("");
                            return self.changed_result();
                        }
                        if let Some(previous) = self.previous_grapheme(start) {
                            self.text.replace_range(previous..start, "");
                            self.caret.byte_index = previous;
                            self.anchor = self.caret;
                            return self.changed_result();
                        }
                    }
                    Key::Named(NamedKey::Other(name)) if name == "Delete" => {
                        handled = true;
                        let (start, end) = self.selection();
                        if start != end {
                            self.replace_selection("");
                            return self.changed_result();
                        }
                        if let Some(next) = self.next_grapheme(end) {
                            self.text.replace_range(end..next, "");
                            return self.changed_result();
                        }
                    }
                    Key::Named(NamedKey::Other(name)) if name == "ArrowLeft" => {
                        self.move_caret(CaretMovement::VisualLeft, modifiers.shift);
                        handled = true;
                    }
                    Key::Named(NamedKey::Other(name)) if name == "ArrowRight" => {
                        self.move_caret(CaretMovement::VisualRight, modifiers.shift);
                        handled = true;
                    }
                    Key::Named(NamedKey::Other(name)) if name == "Home" => {
                        self.move_caret(CaretMovement::LineStart, modifiers.shift);
                        handled = true;
                    }
                    Key::Named(NamedKey::Other(name)) if name == "End" => {
                        self.move_caret(CaretMovement::LineEnd, modifiers.shift);
                        handled = true;
                    }
                    Key::Named(NamedKey::Enter) => {
                        let Some(callback) = &self.submitted else {
                            return EventResult::default();
                        };
                        return EventResult {
                            action: Some(callback(self.text.clone())),
                            handled: true,
                            ..EventResult::default()
                        };
                    }
                    _ if !command && !modifiers.alt => {
                        if let Some(text) = input.text.as_deref()
                            && !text.chars().any(char::is_control)
                        {
                            self.replace_selection(text);
                            return self.changed_result();
                        }
                    }
                    _ => {}
                }
                if handled {
                    EventResult {
                        invalidation: Invalidation::PAINT | Invalidation::ACCESSIBILITY,
                        handled: true,
                        ..EventResult::default()
                    }
                } else {
                    EventResult::default()
                }
            }
            UiInput::Ime(ImeEvent::Preedit(value, _)) => {
                self.preedit = value;
                EventResult {
                    invalidation: Invalidation::LAYOUT_ALL,
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::Ime(ImeEvent::Commit(value)) => {
                self.preedit.clear();
                self.replace_selection(&value);
                self.changed_result()
            }
            UiInput::Ime(ImeEvent::Disabled) => {
                self.preedit.clear();
                EventResult {
                    invalidation: Invalidation::LAYOUT_ALL,
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::Paste(value) => {
                self.replace_selection(&value);
                self.changed_result()
            }
            _ => EventResult::default(),
        }
    }

    fn semantic_actions(&self) -> Vec<SemanticActionKind> {
        vec![
            SemanticActionKind::Focus,
            SemanticActionKind::SetText,
            SemanticActionKind::SetSelection,
        ]
    }
    fn semantic_action(&mut self, action: SemanticAction) -> EventResult {
        match action {
            SemanticAction::SetText(text) => {
                self.text = text;
                self.caret.byte_index = self.text.len();
                self.anchor = self.caret;
                self.changed_result()
            }
            SemanticAction::SetSelection { anchor, focus } => {
                self.anchor.byte_index = self.clamp_boundary(anchor);
                self.caret.byte_index = self.clamp_boundary(focus);
                EventResult {
                    invalidation: Invalidation::PAINT | Invalidation::ACCESSIBILITY,
                    handled: true,
                    ..EventResult::default()
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
        CursorIcon::Text
    }
}
