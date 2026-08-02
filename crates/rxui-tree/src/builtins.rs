//! Basic elements used by the research vertical slices.

use std::any::Any;

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
    math::{Affine2, Vec2},
};
use astrelis_paint::{Brush, FillRule, Painter, Path};
use astrelis_platform::{CursorIcon, ElementState, Key, NamedKey};
use astrelis_text::{ParagraphStyle, TextLayout, TextLayoutRequest, TextStyle, TextWrap};

use crate::{
    Constraints, Element, EventResult, Invalidation, LayoutContext, SemanticAction,
    SemanticActionKind, SemanticData, SemanticRole, ShapingMemo, UiInput,
};

fn hover_color(color: Color) -> Color {
    Color::new(
        color.r + (1.0 - color.r) * 0.12,
        color.g + (1.0 - color.g) * 0.12,
        color.b + (1.0 - color.b) * 0.12,
        color.a,
    )
}

/// Main-axis direction for [`Flex`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Axis {
    /// Children advance from left to right.
    Horizontal,
    /// Children advance from top to bottom.
    #[default]
    Vertical,
}

/// Placement of intrinsic content within all available space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    /// Leading edge on both axes.
    TopLeading,
    /// Centered horizontally at the leading vertical edge.
    Top,
    /// Trailing horizontally at the leading vertical edge.
    TopTrailing,
    /// Leading horizontally and centered vertically.
    Leading,
    /// Centered on both axes.
    #[default]
    Center,
    /// Trailing horizontally and centered vertically.
    Trailing,
    /// Leading horizontally at the trailing vertical edge.
    BottomLeading,
    /// Centered horizontally at the trailing vertical edge.
    Bottom,
    /// Trailing edge on both axes.
    BottomTrailing,
}

/// Simple flex container with deterministic retained child layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Flex {
    /// Main axis.
    pub axis: Axis,
    /// Gap between adjacent children.
    pub gap: f32,
    /// Insets inside the container.
    pub padding: f32,
    /// Optional background fill.
    pub background: Option<Color>,
}

impl Default for Flex {
    fn default() -> Self {
        Self {
            axis: Axis::Vertical,
            gap: 0.0,
            padding: 0.0,
            background: None,
        }
    }
}

impl Element for Flex {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let padding = self.padding.max(0.0);
        let inner_max = LogicalSize::new(
            (constraints.max.width - padding * 2.0).max(0.0),
            (constraints.max.height - padding * 2.0).max(0.0),
        );
        let children = context.children();
        let mut sizes = Vec::with_capacity(children.len());
        let mut total_fixed_main = 0.0f32;
        let mut total_grow = 0.0f32;
        for child in children.iter().copied() {
            let grow = context.child_flex_grow(child);
            if grow > 0.0 {
                // Growing children are laid out exactly once below, against their
                // resolved main-axis share. Measuring them here would only pollute
                // their cached constraints, since `total_fixed_main` ignores them
                // and the growth pass overwrites their placeholder size.
                total_grow += grow;
                sizes.push(LogicalSize::ZERO);
                continue;
            }
            let size = context.layout_child(child, Constraints::new(LogicalSize::ZERO, inner_max));
            total_fixed_main += match self.axis {
                Axis::Horizontal => size.width,
                Axis::Vertical => size.height,
            };
            sizes.push(size);
        }
        let gaps = self.gap.max(0.0) * children.len().saturating_sub(1) as f32;
        let available_main = match self.axis {
            Axis::Horizontal => inner_max.width,
            Axis::Vertical => inner_max.height,
        };
        let remaining = (available_main - total_fixed_main - gaps).max(0.0);
        if total_grow > 0.0 {
            for (index, child) in children.iter().copied().enumerate() {
                let grow = context.child_flex_grow(child);
                if grow <= 0.0 {
                    continue;
                }
                let target_main = remaining * grow / total_grow;
                let child_constraints = match self.axis {
                    Axis::Horizontal => Constraints::new(
                        LogicalSize::new(target_main, inner_max.height),
                        LogicalSize::new(target_main, inner_max.height),
                    ),
                    Axis::Vertical => Constraints::new(
                        LogicalSize::new(inner_max.width, target_main),
                        LogicalSize::new(inner_max.width, target_main),
                    ),
                };
                sizes[index] = context.layout_child(child, child_constraints);
            }
        }
        let mut cursor = padding;
        let mut cross = 0.0f32;
        for (child, size) in children.iter().copied().zip(sizes) {
            let origin = match self.axis {
                Axis::Horizontal => LogicalPoint::new(cursor, padding),
                Axis::Vertical => LogicalPoint::new(padding, cursor),
            };
            context.place_child(child, origin);
            match self.axis {
                Axis::Horizontal => {
                    cursor += size.width + self.gap.max(0.0);
                    cross = cross.max(size.height);
                }
                Axis::Vertical => {
                    cursor += size.height + self.gap.max(0.0);
                    cross = cross.max(size.width);
                }
            }
        }
        if !children.is_empty() {
            cursor -= self.gap.max(0.0);
        }
        let desired = match self.axis {
            Axis::Horizontal => LogicalSize::new(cursor + padding, cross + padding * 2.0),
            Axis::Vertical => LogicalSize::new(cross + padding * 2.0, cursor + padding),
        };
        constraints.constrain(desired)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        if let Some(color) = self.background {
            painter.fill_rect(
                LogicalRect::from_xywh(0.0, 0.0, size.width, size.height),
                Brush::Solid(color),
            )?;
        }
        Ok(())
    }

    fn clips_children(&self) -> bool {
        true
    }
}

/// Overlay container which places every child at the same inset origin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack {
    /// Insets inside the container.
    pub padding: f32,
    /// Optional background fill.
    pub background: Option<Color>,
}

impl Element for Stack {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let padding = self.padding.max(0.0);
        let inner_max = LogicalSize::new(
            (constraints.max.width - padding * 2.0).max(0.0),
            (constraints.max.height - padding * 2.0).max(0.0),
        );
        let mut content = LogicalSize::ZERO;
        for child in context.children() {
            let size = context.layout_child(
                child,
                if context.child_flex_grow(child) > 0.0 {
                    Constraints::tight(inner_max)
                } else {
                    Constraints::new(LogicalSize::ZERO, inner_max)
                },
            );
            context.place_child(child, LogicalPoint::new(padding, padding));
            content.width = content.width.max(size.width);
            content.height = content.height.max(size.height);
        }
        constraints.constrain(LogicalSize::new(
            content.width + padding * 2.0,
            content.height + padding * 2.0,
        ))
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        if let Some(color) = self.background {
            painter.fill_rect(
                LogicalRect::from_xywh(0.0, 0.0, size.width, size.height),
                Brush::Solid(color),
            )?;
        }
        Ok(())
    }
}

/// Expands to the available size and positions one intrinsic child within it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Align {
    /// Child placement.
    pub alignment: Alignment,
    /// Minimum distance from each boundary.
    pub padding: f32,
}

impl Element for Align {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let padding = self.padding.max(0.0);
        let size = constraints.max;
        let available = LogicalSize::new(
            (size.width - padding * 2.0).max(0.0),
            (size.height - padding * 2.0).max(0.0),
        );
        if let Some(child) = context.children().into_iter().next() {
            let child_size =
                context.layout_child(child, Constraints::new(LogicalSize::ZERO, available));
            let remaining_x = (available.width - child_size.width).max(0.0);
            let remaining_y = (available.height - child_size.height).max(0.0);
            let x = match self.alignment {
                Alignment::TopLeading | Alignment::Leading | Alignment::BottomLeading => 0.0,
                Alignment::Top | Alignment::Center | Alignment::Bottom => remaining_x * 0.5,
                Alignment::TopTrailing | Alignment::Trailing | Alignment::BottomTrailing => {
                    remaining_x
                }
            };
            let y = match self.alignment {
                Alignment::TopLeading | Alignment::Top | Alignment::TopTrailing => 0.0,
                Alignment::Leading | Alignment::Center | Alignment::Trailing => remaining_y * 0.5,
                Alignment::BottomLeading | Alignment::Bottom | Alignment::BottomTrailing => {
                    remaining_y
                }
            };
            context.place_child(child, LogicalPoint::new(padding + x, padding + y));
        }
        constraints.constrain(size)
    }

    fn clips_children(&self) -> bool {
        true
    }
}

type KeyAction = dyn Fn() -> Box<dyn Any>;

/// Transparent keyboard-bubbling boundary for overlay and command handling.
pub struct KeyListener {
    escape: Option<Box<KeyAction>>,
    previous: Option<Box<KeyAction>>,
    next: Option<Box<KeyAction>>,
    submit: Option<Box<KeyAction>>,
}

impl KeyListener {
    /// Creates a keyboard boundary with an Escape action.
    pub fn on_escape(action: impl Fn() -> Box<dyn Any> + 'static) -> Self {
        Self {
            escape: Some(Box::new(action)),
            previous: None,
            next: None,
            submit: None,
        }
    }

    /// Replaces the Escape action.
    pub fn set_escape(&mut self, action: impl Fn() -> Box<dyn Any> + 'static) {
        self.escape = Some(Box::new(action));
    }

    /// Creates a keyboard boundary for list navigation and submission.
    pub fn command_navigation(
        previous: impl Fn() -> Box<dyn Any> + 'static,
        next: impl Fn() -> Box<dyn Any> + 'static,
        submit: impl Fn() -> Box<dyn Any> + 'static,
    ) -> Self {
        Self {
            escape: None,
            previous: Some(Box::new(previous)),
            next: Some(Box::new(next)),
            submit: Some(Box::new(submit)),
        }
    }

    /// Replaces list-navigation and submission actions.
    pub fn set_command_navigation(
        &mut self,
        previous: impl Fn() -> Box<dyn Any> + 'static,
        next: impl Fn() -> Box<dyn Any> + 'static,
        submit: impl Fn() -> Box<dyn Any> + 'static,
    ) {
        self.previous = Some(Box::new(previous));
        self.next = Some(Box::new(next));
        self.submit = Some(Box::new(submit));
    }
}

impl Element for KeyListener {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let Some(child) = context.children().into_iter().next() else {
            return constraints.constrain(LogicalSize::ZERO);
        };
        let size = context.layout_child(child, constraints);
        context.place_child(child, LogicalPoint::ZERO);
        constraints.constrain(size)
    }
    fn event(&mut self, input: UiInput) -> EventResult {
        if let UiInput::Keyboard { input, .. } = input
            && input.state == ElementState::Pressed
        {
            let action = match &input.logical_key {
                Key::Named(NamedKey::Escape) => self.escape.as_ref(),
                Key::Named(NamedKey::Enter) => self.submit.as_ref(),
                Key::Named(NamedKey::Other(name)) if name == "ArrowUp" => self.previous.as_ref(),
                Key::Named(NamedKey::Other(name)) if name == "ArrowDown" => self.next.as_ref(),
                _ => None,
            };
            if let Some(action) = action {
                return EventResult {
                    action: Some(action()),
                    handled: true,
                    ..EventResult::default()
                };
            }
        }
        EventResult::default()
    }
}

type SplitAction = dyn Fn(f32) -> Box<dyn Any>;

/// Two-child resizing container with a dedicated draggable divider.
pub struct SplitPane {
    /// Split direction.
    pub axis: Axis,
    /// Fraction of content space assigned to the first child.
    pub ratio: f32,
    /// Visible and interactive divider thickness.
    pub divider_extent: f32,
    /// Divider color.
    pub divider_color: Color,
    size: LogicalSize,
    dragging: bool,
    hovered: bool,
    drag_offset: f32,
    changed: Box<SplitAction>,
}

impl SplitPane {
    /// Creates a controlled split pane.
    pub fn new(axis: Axis, ratio: f32, changed: impl Fn(f32) -> Box<dyn Any> + 'static) -> Self {
        Self {
            axis,
            ratio: ratio.clamp(0.05, 0.95),
            divider_extent: 6.0,
            divider_color: Color::new(0.18, 0.2, 0.24, 1.0),
            size: LogicalSize::ZERO,
            dragging: false,
            hovered: false,
            drag_offset: 0.0,
            changed: Box::new(changed),
        }
    }
    /// Replaces the controlled resize action.
    pub fn set_changed(&mut self, changed: impl Fn(f32) -> Box<dyn Any> + 'static) {
        self.changed = Box::new(changed);
    }
    fn main_extent(&self) -> f32 {
        match self.axis {
            Axis::Horizontal => self.size.width,
            Axis::Vertical => self.size.height,
        }
    }
    fn divider_rect(&self) -> LogicalRect {
        let divider = self.divider_extent.max(1.0).min(self.main_extent());
        let content = (self.main_extent() - divider).max(0.0);
        let first = content * self.ratio.clamp(0.05, 0.95);
        match self.axis {
            Axis::Horizontal => LogicalRect::from_xywh(first, 0.0, divider, self.size.height),
            Axis::Vertical => LogicalRect::from_xywh(0.0, first, self.size.width, divider),
        }
    }
    fn set_from_point(&mut self, point: LogicalPoint) -> EventResult {
        let divider = self.divider_extent.max(1.0).min(self.main_extent());
        let content = (self.main_extent() - divider).max(1.0);
        let coordinate = match self.axis {
            Axis::Horizontal => point.x - divider * 0.5 - self.drag_offset,
            Axis::Vertical => point.y - divider * 0.5 - self.drag_offset,
        };
        self.ratio = (coordinate / content).clamp(0.05, 0.95);
        EventResult {
            action: Some((self.changed)(self.ratio)),
            invalidation: Invalidation::LAYOUT_ALL,
            handled: true,
            ..EventResult::default()
        }
    }
}

impl Element for SplitPane {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        self.size = constraints.max;
        let divider = self.divider_extent.max(1.0).min(self.main_extent());
        let content = (self.main_extent() - divider).max(0.0);
        let first_main = content * self.ratio.clamp(0.05, 0.95);
        let second_main = content - first_main;
        let children = context.children();
        if let Some(first) = children.first().copied() {
            let size = match self.axis {
                Axis::Horizontal => LogicalSize::new(first_main, self.size.height),
                Axis::Vertical => LogicalSize::new(self.size.width, first_main),
            };
            context.layout_child(first, Constraints::tight(size));
            context.place_child(first, LogicalPoint::ZERO);
        }
        if let Some(second) = children.get(1).copied() {
            let size = match self.axis {
                Axis::Horizontal => LogicalSize::new(second_main, self.size.height),
                Axis::Vertical => LogicalSize::new(self.size.width, second_main),
            };
            let origin = match self.axis {
                Axis::Horizontal => LogicalPoint::new(first_main + divider, 0.0),
                Axis::Vertical => LogicalPoint::new(0.0, first_main + divider),
            };
            context.layout_child(second, Constraints::tight(size));
            context.place_child(second, origin);
        }
        constraints.constrain(self.size)
    }
    fn paint(
        &self,
        painter: &mut Painter,
        _size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        painter.fill_rect(
            self.divider_rect(),
            Brush::Solid(if self.dragging || self.hovered {
                hover_color(self.divider_color)
            } else {
                self.divider_color
            }),
        )
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
            UiInput::PointerPressed(point) if self.divider_rect().contains(point) => {
                self.dragging = true;
                let divider = self.divider_rect();
                self.drag_offset = match self.axis {
                    Axis::Horizontal => point.x - (divider.origin.x + divider.size.width * 0.5),
                    Axis::Vertical => point.y - (divider.origin.y + divider.size.height * 0.5),
                };
                EventResult {
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::PointerMoved(point) if self.dragging => self.set_from_point(point),
            UiInput::PointerReleased(point) if self.dragging => {
                self.dragging = false;
                self.set_from_point(point)
            }
            _ => EventResult::default(),
        }
    }
    fn hit_test(&self, point: LogicalPoint, _size: LogicalSize) -> bool {
        self.divider_rect().contains(point)
    }
    fn hit_testable(&self) -> bool {
        true
    }
    fn cursor_icon(&self) -> CursorIcon {
        match self.axis {
            Axis::Horizontal => CursorIcon::EwResize,
            Axis::Vertical => CursorIcon::NsResize,
        }
    }
    fn clips_children(&self) -> bool {
        true
    }
}

/// Explicit sizing and flex-growth boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Optional preferred width.
    pub width: Option<f32>,
    /// Optional preferred height.
    pub height: Option<f32>,
    /// Minimum accepted size.
    pub min: LogicalSize,
    /// Optional maximum size.
    pub max: Option<LogicalSize>,
    /// Relative main-axis growth in a [`Flex`] parent.
    pub grow: f32,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            min: LogicalSize::ZERO,
            max: None,
            grow: 0.0,
        }
    }
}

impl Element for Frame {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let configured_max = self.max.unwrap_or(constraints.max);
        let maximum = LogicalSize::new(
            configured_max.width.min(constraints.max.width),
            configured_max.height.min(constraints.max.height),
        );
        let minimum = LogicalSize::new(
            self.min.width.max(constraints.min.width).min(maximum.width),
            self.min
                .height
                .max(constraints.min.height)
                .min(maximum.height),
        );
        let children = context.children();
        // Measured content only feeds an axis without an explicit extent, and a
        // pinned axis clamps to one value regardless of the measurement. So when
        // both axes are pinned - by `width`/`height` or by a tight incoming
        // constraint, which is what a growing flex parent supplies - the
        // measuring pass cannot affect `size` and is pure duplicate work.
        let pinned_width = self.width.is_some() || minimum.width == maximum.width;
        let pinned_height = self.height.is_some() || minimum.height == maximum.height;
        let mut content = LogicalSize::ZERO;
        if !pinned_width || !pinned_height {
            for child in children.iter().copied() {
                let size =
                    context.layout_child(child, Constraints::new(LogicalSize::ZERO, maximum));
                content.width = content.width.max(size.width);
                content.height = content.height.max(size.height);
            }
        }
        let size = Constraints::new(minimum, maximum).constrain(LogicalSize::new(
            self.width.unwrap_or(content.width),
            self.height.unwrap_or(content.height),
        ));
        for child in children {
            context.layout_child(child, Constraints::tight(size));
            context.place_child(child, LogicalPoint::ZERO);
        }
        size
    }

    fn flex_grow(&self) -> f32 {
        self.grow.max(0.0)
    }
}

/// Fixed or constraint-filling visual box.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxElement {
    /// Preferred size.
    pub size: LogicalSize,
    /// Background color.
    pub color: Color,
    /// Accessible properties.
    pub semantics: Option<SemanticData>,
    /// Whether pointer input targets this box.
    pub interactive: bool,
}

impl BoxElement {
    /// Creates a colored fixed-size box.
    pub fn new(size: LogicalSize, color: Color) -> Self {
        Self {
            size,
            color,
            semantics: None,
            interactive: false,
        }
    }
}

impl Element for BoxElement {
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
        painter.fill_rect(
            LogicalRect::from_xywh(0.0, 0.0, size.width, size.height),
            Brush::Solid(self.color),
        )?;
        Ok(())
    }

    fn accessibility(&self) -> Option<SemanticData> {
        self.semantics.clone()
    }

    fn hit_testable(&self) -> bool {
        self.interactive
    }
}

/// A box that owns one typed, reusable activation payload.
///
/// Frameworks replace the payload while reconciling a freshly described
/// listener. Replacing it requests no retained pass: routing data changes
/// neither layout nor pixels, hit testing, or accessibility.
pub struct ActionBox<A: 'static> {
    /// Visual, semantic, and hit-test properties.
    pub surface: BoxElement,
    /// Payload cloned by every pointer or semantic activation.
    pub action: Option<A>,
}

impl<A: 'static> ActionBox<A> {
    /// Creates an action-bearing box.
    pub fn new(surface: BoxElement, action: Option<A>) -> Self {
        Self { surface, action }
    }
}

impl<A: Clone + 'static> Element for ActionBox<A> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        self.surface.layout(context, constraints)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        self.surface.paint(painter, size)
    }

    fn accessibility(&self) -> Option<SemanticData> {
        self.surface.accessibility()
    }

    fn event(&mut self, input: UiInput) -> EventResult {
        if matches!(input, UiInput::PointerReleased(_)) {
            return self
                .action
                .clone()
                .map(EventResult::action)
                .unwrap_or_default();
        }
        EventResult::default()
    }

    fn semantic_actions(&self) -> Vec<SemanticActionKind> {
        vec![SemanticActionKind::Focus, SemanticActionKind::Activate]
    }

    fn semantic_action(&mut self, action: SemanticAction) -> EventResult {
        if matches!(action, SemanticAction::Activate) {
            return self
                .action
                .clone()
                .map(EventResult::action)
                .unwrap_or_default();
        }
        EventResult::default()
    }

    fn hit_testable(&self) -> bool {
        self.surface.interactive
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// Retained shaped text label.
#[derive(Clone, Debug)]
pub struct Label {
    /// Display and accessible text.
    pub text: String,
    /// Nominal font size used for intrinsic measurement.
    pub font_size: f32,
    /// Optional paint color for a deterministic placeholder glyph bar.
    pub color: Option<Color>,
    /// Whether this painted text publishes its own accessible label.
    pub semantic: bool,
    preferred_width: Option<f32>,
    layout: Option<TextLayout>,
    shaped: ShapingMemo,
}

impl Label {
    /// Creates a label.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font_size: 14.0,
            color: Some(Color::WHITE),
            semantic: true,
            preferred_width: None,
            layout: None,
            shaped: ShapingMemo::default(),
        }
    }

    /// Sets the label's font size.
    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    /// Sets the label's glyph color.
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Constrains wrapping and reserves a preferred width.
    pub fn with_width(mut self, width: f32) -> Self {
        self.preferred_width = Some(width.max(0.0));
        self
    }

    /// Keeps the text visible while suppressing its accessibility node.
    ///
    /// Use this when an enclosing semantic control already carries the same
    /// accessible name, such as the painted label inside an action box.
    pub fn without_semantics(mut self) -> Self {
        self.semantic = false;
        self
    }

    /// Changes the optional preferred width.
    pub fn set_width(&mut self, width: Option<f32>) {
        self.preferred_width = width.map(|width| width.max(0.0));
    }

    /// Returns the normalized preferred width.
    pub const fn width(&self) -> Option<f32> {
        self.preferred_width
    }
}

impl Element for Label {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let mut request = TextLayoutRequest::new(self.text.clone());
        request.style = TextStyle {
            size: self.font_size.max(1.0),
            color: self.color.unwrap_or(Color::WHITE),
            ..TextStyle::default()
        };
        let max_width = self
            .preferred_width
            .unwrap_or(constraints.max.width)
            .min(constraints.max.width);
        request.paragraph = ParagraphStyle {
            max_width: Some(max_width),
            wrap: TextWrap::Wrap,
            ..ParagraphStyle::default()
        };
        let layout = self.shaped.shape(context, request);
        let mut desired = layout.size();
        if let Some(width) = self.preferred_width {
            desired.width = width;
        }
        let size = constraints.constrain(desired);
        self.layout = Some(layout);
        size
    }

    fn paint(
        &self,
        painter: &mut Painter,
        _size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        if let Some(layout) = &self.layout {
            painter.draw_text(layout, LogicalPoint::ZERO, 1.0)?;
        }
        Ok(())
    }

    fn accessibility(&self) -> Option<SemanticData> {
        self.semantic.then(|| SemanticData {
            role: SemanticRole::Label,
            label: self.text.clone(),
            ..SemanticData::default()
        })
    }
}

/// Vector content for a button.
#[derive(Clone, Debug)]
pub struct ButtonIcon {
    /// Immutable monochrome vector path.
    pub path: Path,
    /// Coordinate system used by the vector path.
    pub view_box: LogicalSize,
    /// Requested logical square edge.
    pub size: f32,
    /// Path winding interpretation.
    pub fill_rule: FillRule,
}

impl ButtonIcon {
    /// Creates vector content for a button.
    pub const fn new(path: Path, view_box: LogicalSize, size: f32) -> Self {
        Self {
            path,
            view_box,
            size,
            fill_rule: FillRule::NonZero,
        }
    }
    /// Selects path winding interpretation.
    pub const fn with_fill_rule(mut self, fill_rule: FillRule) -> Self {
        self.fill_rule = fill_rule;
        self
    }
}

/// Minimal activatable control for action-routing tests.
pub struct Button {
    /// Accessible label.
    pub label: String,
    /// Preferred size.
    pub size: LogicalSize,
    /// Normal background.
    pub color: Color,
    /// Pressed background.
    pub pressed_color: Color,
    /// Glyph color.
    pub text_color: Color,
    /// Glyph size.
    pub font_size: f32,
    /// Whether the accessible label is also painted.
    pub show_label: bool,
    /// Optional leading vector glyph.
    pub icon: Option<ButtonIcon>,
    pressed: bool,
    hovered: bool,
    resolved_size: LogicalSize,
    layout: Option<TextLayout>,
    shaped: ShapingMemo,
    action: Option<Box<dyn Fn() -> Box<dyn Any>>>,
}

impl Button {
    /// Creates a button which emits `action` on release.
    pub fn new<A: Any + Clone>(
        label: impl Into<String>,
        size: LogicalSize,
        color: Color,
        pressed_color: Color,
        action: A,
    ) -> Self {
        Self {
            label: label.into(),
            size,
            color,
            pressed_color,
            text_color: Color::WHITE,
            font_size: 14.0,
            show_label: true,
            icon: None,
            pressed: false,
            hovered: false,
            resolved_size: LogicalSize::ZERO,
            layout: None,
            shaped: ShapingMemo::default(),
            action: Some(Box::new(move || Box::new(action.clone()))),
        }
    }

    /// Creates a button backed by an erased action factory.
    pub fn with_action_factory(
        label: impl Into<String>,
        size: LogicalSize,
        color: Color,
        pressed_color: Color,
        action: impl Fn() -> Box<dyn Any> + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            size,
            color,
            pressed_color,
            text_color: Color::WHITE,
            font_size: 14.0,
            show_label: true,
            icon: None,
            pressed: false,
            hovered: false,
            resolved_size: LogicalSize::ZERO,
            layout: None,
            shaped: ShapingMemo::default(),
            action: Some(Box::new(action)),
        }
    }

    /// Replaces the erased action factory without recreating the control.
    pub fn set_action_factory(&mut self, action: impl Fn() -> Box<dyn Any> + 'static) {
        self.action = Some(Box::new(action));
    }
    /// Adds or replaces leading vector content.
    pub fn with_icon(mut self, icon: ButtonIcon) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Replaces optional leading vector content.
    pub fn set_icon(&mut self, icon: Option<ButtonIcon>) {
        self.icon = icon;
    }
    /// Selects whether the accessible label is also painted.
    pub const fn with_label_visible(mut self, visible: bool) -> Self {
        self.show_label = visible;
        self
    }
    /// Selects whether the accessible label is also painted.
    pub fn set_label_visible(&mut self, visible: bool) {
        self.show_label = visible;
    }
}

impl Element for Button {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        self.layout = if self.label.is_empty() || !self.show_label {
            None
        } else {
            let mut request = TextLayoutRequest::new(self.label.clone());
            request.style = TextStyle {
                size: self.font_size.max(1.0),
                color: self.text_color,
                ..TextStyle::default()
            };
            request.paragraph.wrap = TextWrap::NoWrap;
            Some(self.shaped.shape(context, request))
        };
        let icon_size = self
            .icon
            .as_ref()
            .map(|icon| {
                if icon.size.is_finite() {
                    icon.size.max(1.0)
                } else {
                    16.0
                }
            })
            .unwrap_or(0.0);
        let text_size = self
            .layout
            .as_ref()
            .map(TextLayout::size)
            .unwrap_or(LogicalSize::ZERO);
        let gap = if self.icon.is_some() && self.layout.is_some() {
            6.0
        } else {
            0.0
        };
        let horizontal_padding = if self.layout.is_some() { 24.0 } else { 12.0 };
        let intrinsic = LogicalSize::new(
            icon_size + gap + text_size.width + horizontal_padding,
            icon_size.max(text_size.height) + 12.0,
        );
        self.resolved_size = constraints.constrain(LogicalSize::new(
            self.size.width.max(intrinsic.width),
            self.size.height.max(intrinsic.height),
        ));
        self.resolved_size
    }
    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        painter.fill_rect(
            LogicalRect::from_xywh(0.0, 0.0, size.width, size.height),
            Brush::Solid(if self.pressed {
                self.pressed_color
            } else if self.hovered {
                hover_color(self.color)
            } else {
                self.color
            }),
        )?;
        let icon_size = self
            .icon
            .as_ref()
            .map(|icon| {
                if icon.size.is_finite() {
                    icon.size.max(1.0)
                } else {
                    16.0
                }
            })
            .unwrap_or(0.0);
        let text_width = self
            .layout
            .as_ref()
            .map(|layout| layout.size().width)
            .unwrap_or(0.0);
        let gap = if self.icon.is_some() && self.layout.is_some() {
            6.0
        } else {
            0.0
        };
        let content_width = icon_size + gap + text_width;
        let content_x = (size.width - content_width).max(0.0) * 0.5;
        if let Some(icon) = &self.icon {
            let view_width = icon.view_box.width.max(f32::EPSILON);
            let view_height = icon.view_box.height.max(f32::EPSILON);
            let scale = (icon_size / view_width).min(icon_size / view_height);
            let offset = LogicalPoint::new(
                content_x + (icon_size - icon.view_box.width * scale) * 0.5,
                (size.height - icon.view_box.height * scale).max(0.0) * 0.5,
            );
            painter.with_save(|painter| {
                painter.transform(
                    Affine2::from_translation(Vec2::new(offset.x, offset.y))
                        * Affine2::from_scale(Vec2::splat(scale)),
                )?;
                painter.fill_path(&icon.path, icon.fill_rule, Brush::Solid(self.text_color))
            })?;
        }
        if let Some(layout) = &self.layout {
            painter.draw_text(
                layout,
                LogicalPoint::new(
                    content_x + icon_size + gap,
                    (size.height - layout.size().height).max(0.0) * 0.5,
                ),
                1.0,
            )?;
        }
        Ok(())
    }
    fn accessibility(&self) -> Option<SemanticData> {
        Some(SemanticData {
            role: SemanticRole::Button,
            label: self.label.clone(),
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
            UiInput::PointerPressed(_) => {
                self.pressed = true;
                EventResult {
                    invalidation: Invalidation::PAINT,
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::PointerReleased(point) if self.pressed => {
                self.pressed = false;
                EventResult {
                    action: LogicalRect::from_xywh(
                        0.0,
                        0.0,
                        self.resolved_size.width,
                        self.resolved_size.height,
                    )
                    .contains(point)
                    .then(|| self.action.as_ref().map(|action| action()))
                    .flatten(),
                    invalidation: Invalidation::PAINT,
                    clipboard: None,
                    handled: true,
                }
            }
            UiInput::Keyboard { input, .. }
                if input.state == ElementState::Pressed
                    && matches!(
                        input.logical_key,
                        Key::Named(NamedKey::Enter | NamedKey::Space)
                    ) =>
            {
                EventResult {
                    action: self.action.as_ref().map(|action| action()),
                    handled: true,
                    ..EventResult::default()
                }
            }
            _ => EventResult::default(),
        }
    }
    fn semantic_actions(&self) -> Vec<SemanticActionKind> {
        vec![SemanticActionKind::Focus, SemanticActionKind::Activate]
    }
    fn semantic_action(&mut self, action: SemanticAction) -> EventResult {
        match action {
            SemanticAction::Activate => EventResult {
                action: self.action.as_ref().map(|action| action()),
                handled: true,
                ..EventResult::default()
            },
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
