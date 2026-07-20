//! Theme-aware vector icons and icon buttons.

use std::{error::Error, fmt};

use astrelis_core::{
    color::Color,
    geometry::{LogicalRect, LogicalSize, Point, Size},
    math::{Affine2, Vec2},
};
use astrelis_paint::{Brush, CornerRadii, FillRule, Painter, Path, PathVerb, RoundedRect};
use astrelis_platform::{CursorIcon, ElementState, Key, NamedKey, PointerButton};
use astrelis_ui::widget_any;
use astrelis_ui_core::{
    EventContext, MountContext, RoutedEvent, RoutedEventKind, SemanticAction, SemanticActionKind,
    SemanticRole, Theme, UiError, Widget, WidgetContainerStyle,
};

/// Logical glyph size icons are painted at inside buttons.
const ICON_SIZE: f32 = 16.0;

/// Validated monochrome vector icon.
#[derive(Clone, Debug)]
pub struct Icon {
    view_box: LogicalSize,
    path: Path,
}

impl Icon {
    /// Creates an icon from an immutable Astrelis paint path.
    pub fn new(view_box: LogicalSize, path: Path) -> Result<Self, IconError> {
        if !view_box.width.is_finite()
            || !view_box.height.is_finite()
            || view_box.width <= 0.0
            || view_box.height <= 0.0
        {
            return Err(IconError::new(
                "icon view-box dimensions must be finite and positive",
            ));
        }
        if path.is_empty() {
            return Err(IconError::new("icon paths cannot be empty"));
        }
        Ok(Self { view_box, path })
    }

    /// Builds an icon from path verbs.
    pub fn from_verbs(
        view_box: LogicalSize,
        verbs: impl IntoIterator<Item = PathVerb>,
    ) -> Result<Self, IconError> {
        let mut builder = Path::builder();
        for verb in verbs {
            match verb {
                PathVerb::MoveTo(point) => builder.move_to(point),
                PathVerb::LineTo(point) => builder.line_to(point),
                PathVerb::QuadTo(control, point) => builder.quad_to(control, point),
                PathVerb::CubicTo(first, second, point) => builder.cubic_to(first, second, point),
                PathVerb::Close => builder.close(),
            }
            .map_err(IconError::new)?;
        }
        Self::new(view_box, builder.finish())
    }

    /// Returns the icon coordinate system.
    pub const fn view_box(&self) -> LogicalSize {
        self.view_box
    }

    /// Returns the immutable vector path.
    pub const fn path(&self) -> &Path {
        &self.path
    }
}

/// Invalid custom vector icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconError(String);

impl IconError {
    fn new(message: impl fmt::Display) -> Self {
        Self(message.to_string())
    }
}

impl fmt::Display for IconError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for IconError {}

/// Non-interactive icon retained in a UI tree.
pub struct IconView {
    icon: Icon,
    size: f32,
    color: Option<Color>,
    label: Option<String>,
}

impl IconView {
    /// Creates a theme-colored icon.
    pub fn new(icon: Icon, size: f32) -> Self {
        Self {
            icon,
            size: valid_size(size),
            color: None,
            label: None,
        }
    }

    /// Overrides the theme foreground color.
    pub const fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Exposes an optional semantic image label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl<Message: 'static> Widget<Message> for IconView {
    widget_any!();

    fn intrinsic_size(&self, _theme: &Theme) -> LogicalSize {
        Size::new(self.size, self.size)
    }

    fn container_style(&self, _theme: &Theme) -> WidgetContainerStyle {
        WidgetContainerStyle::structural()
    }

    fn paint(
        &self,
        painter: &mut Painter,
        bounds: LogicalRect,
        theme: &Theme,
    ) -> Result<(), UiError> {
        paint_icon(
            painter,
            &self.icon,
            bounds,
            self.color.unwrap_or(theme.foreground),
        )
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        self.label
            .as_ref()
            .map(|label| (SemanticRole::Group, label.clone(), None))
    }
}

/// Accessible retained button painted with an icon and optional text child.
pub struct IconButton<Message> {
    icon: Icon,
    label: String,
    message: Message,
    icon_only: bool,
    hovered: bool,
    pressed: bool,
    focused: bool,
}

/// Command-oriented button with optional icon, live label, and checked state.
pub struct CommandButton<Message> {
    icon: Option<Icon>,
    label: String,
    label_handle: Option<astrelis_ui_core::ElementHandle<astrelis_ui_core::Label>>,
    message: Message,
    show_label: bool,
    checked: bool,
    enabled: bool,
    hovered: bool,
    pressed: bool,
    focused: bool,
}

impl<Message> CommandButton<Message> {
    /// Creates a text command button.
    pub fn new(label: impl Into<String>, message: Message) -> Self {
        Self {
            icon: None,
            label: label.into(),
            label_handle: None,
            message,
            show_label: true,
            checked: false,
            enabled: true,
            hovered: false,
            pressed: false,
            focused: false,
        }
    }

    /// Adds a leading vector icon.
    pub fn icon(mut self, icon: Option<Icon>) -> Self {
        self.icon = icon;
        self
    }

    /// Selects icon-only or icon-and-label presentation.
    pub const fn show_label(mut self, show: bool) -> Self {
        self.show_label = show;
        self
    }

    /// Sets initial checked presentation.
    pub const fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Sets initial enabled presentation.
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Updates state mirrored from a command registry.
    pub fn sync(&mut self, label: impl Into<String>, enabled: bool, checked: bool) {
        self.label = label.into();
        self.enabled = enabled;
        self.checked = checked;
        if !enabled {
            self.pressed = false;
            self.hovered = false;
        }
    }

    /// Returns the retained visible-label handle when labels are enabled.
    pub const fn label_handle(
        &self,
    ) -> Option<astrelis_ui_core::ElementHandle<astrelis_ui_core::Label>> {
        self.label_handle
    }
}

impl<Message: Clone + 'static> Widget<Message> for CommandButton<Message> {
    widget_any!();

    fn mounted(&mut self, context: &mut MountContext<'_, Message>) -> Result<(), UiError> {
        if self.show_label {
            self.label_handle = Some(context.add_label(self.label.clone())?);
        }
        Ok(())
    }

    fn intrinsic_size(&self, _theme: &Theme) -> LogicalSize {
        Size::new(
            if self.show_label {
                if self.icon.is_some() { 40.0 } else { 20.0 }
            } else {
                28.0
            },
            28.0,
        )
    }

    fn container_style(&self, theme: &Theme) -> WidgetContainerStyle {
        let padding = theme.control_padding;
        WidgetContainerStyle {
            padding: astrelis_ui_core::Insets {
                left: if self.icon.is_some() {
                    padding.left + ICON_SIZE + theme.spacing.xs
                } else {
                    padding.left
                },
                top: padding.top,
                right: padding.right,
                bottom: padding.bottom,
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
                    context.emit(self.message.clone());
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
                context.emit(self.message.clone());
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
        let checked_accent = self.checked && self.enabled;
        let background = if checked_accent {
            theme.accent
        } else {
            theme.button.resolve(astrelis_ui_core::ControlState {
                enabled: self.enabled,
                hovered: self.hovered,
                pressed: self.pressed,
            })
        };
        let rounded =
            RoundedRect::new(bounds, CornerRadii::uniform(theme.radii.md)).map_err(ui_error)?;
        painter
            .fill_rounded_rect(rounded, Brush::Solid(background))
            .map_err(ui_error)?;
        if let Some(icon) = &self.icon {
            let icon_bounds = icon_slot(bounds, theme.control_padding, false);
            paint_icon(
                painter,
                icon,
                icon_bounds,
                if checked_accent {
                    theme.accent_foreground
                } else if self.enabled {
                    theme.foreground
                } else {
                    theme.disabled_foreground
                },
            )?;
        }
        if self.focused {
            painter
                .stroke_rounded_rect(
                    rounded,
                    astrelis_paint::StrokeStyle {
                        width: theme.metrics.focus_ring,
                        ..Default::default()
                    },
                    Brush::Solid(theme.accent),
                )
                .map_err(ui_error)?;
        }
        Ok(())
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        Some((
            SemanticRole::Button,
            self.label.clone(),
            self.checked.then(|| "checked".into()),
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
                context.emit(self.message.clone());
                true
            }
            _ => false,
        }
    }
}

impl<Message> IconButton<Message> {
    /// Creates an icon-only button. `label` is always used for accessibility.
    pub fn icon_only(icon: Icon, label: impl Into<String>, message: Message) -> Self {
        Self {
            icon,
            label: label.into(),
            message,
            icon_only: true,
            hovered: false,
            pressed: false,
            focused: false,
        }
    }

    /// Creates a button that displays both icon and label.
    pub fn labelled(icon: Icon, label: impl Into<String>, message: Message) -> Self {
        Self {
            icon_only: false,
            ..Self::icon_only(icon, label, message)
        }
    }
}

impl<Message: Clone + 'static> Widget<Message> for IconButton<Message> {
    widget_any!();

    fn mounted(&mut self, context: &mut MountContext<'_, Message>) -> Result<(), UiError> {
        if !self.icon_only {
            context.add_label(self.label.clone())?;
        }
        Ok(())
    }

    fn intrinsic_size(&self, _theme: &Theme) -> LogicalSize {
        if self.icon_only {
            Size::new(28.0, 28.0)
        } else {
            Size::new(40.0, 28.0)
        }
    }

    fn container_style(&self, theme: &Theme) -> WidgetContainerStyle {
        let padding = theme.control_padding;
        WidgetContainerStyle {
            padding: astrelis_ui_core::Insets {
                left: if self.icon_only {
                    padding.left
                } else {
                    padding.left + ICON_SIZE + theme.spacing.xs
                },
                top: padding.top,
                right: padding.right,
                bottom: padding.bottom,
            },
            gap: theme.spacing.sm,
        }
    }

    fn event(&mut self, context: &mut EventContext<'_, Message>, event: &RoutedEvent) {
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
                    context.emit(self.message.clone());
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
                context.emit(self.message.clone());
                context.prevent_default();
            }
            RoutedEventKind::FocusChanged(focused) => {
                self.focused = *focused;
                context.request_paint();
            }
            _ => {}
        }
    }

    fn hit_testable(&self) -> bool {
        true
    }

    fn focusable(&self) -> bool {
        true
    }

    fn cursor_icon(&self) -> Option<CursorIcon> {
        Some(CursorIcon::Pointer)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        bounds: LogicalRect,
        theme: &Theme,
    ) -> Result<(), UiError> {
        let background = theme.button.resolve(astrelis_ui_core::ControlState {
            enabled: true,
            hovered: self.hovered,
            pressed: self.pressed,
        });
        let rounded =
            RoundedRect::new(bounds, CornerRadii::uniform(theme.radii.md)).map_err(ui_error)?;
        painter
            .fill_rounded_rect(rounded, Brush::Solid(background))
            .map_err(ui_error)?;
        let icon_bounds = icon_slot(bounds, theme.control_padding, self.icon_only);
        paint_icon(painter, &self.icon, icon_bounds, theme.foreground)?;
        if self.focused {
            painter
                .stroke_rounded_rect(
                    rounded,
                    astrelis_paint::StrokeStyle {
                        width: theme.metrics.focus_ring,
                        ..Default::default()
                    },
                    Brush::Solid(theme.accent),
                )
                .map_err(ui_error)?;
        }
        Ok(())
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        Some((SemanticRole::Button, self.label.clone(), None))
    }

    fn semantic_actions(&self) -> Vec<SemanticActionKind> {
        vec![SemanticActionKind::Focus, SemanticActionKind::Activate]
    }

    fn semantic_action(
        &mut self,
        context: &mut EventContext<'_, Message>,
        action: &SemanticAction,
    ) -> bool {
        match action {
            SemanticAction::Focus => {
                context.request_focus();
                true
            }
            SemanticAction::Activate => {
                context.emit(self.message.clone());
                true
            }
            _ => false,
        }
    }
}

fn valid_size(size: f32) -> f32 {
    if size.is_finite() && size > 0.0 {
        size
    } else {
        16.0
    }
}

/// Computes where a button paints its icon glyph.
///
/// Labelled buttons reserve a fixed left slot so text lines up after the icon;
/// icon-only buttons center the glyph in both axes — anchoring it to the left
/// slot instead is what previously skewed square buttons like tree disclosures
/// and close buttons.
fn icon_slot(
    bounds: LogicalRect,
    padding: astrelis_ui_core::Insets,
    icon_only: bool,
) -> LogicalRect {
    let vertical = (bounds.size.height - padding.top - padding.bottom).max(0.0);
    let size = if icon_only {
        // Square buttons use the tighter vertical inset on both axes; the
        // horizontal control padding is sized for text, not glyph margins.
        ICON_SIZE
            .min(vertical)
            .min((bounds.size.width - padding.top - padding.bottom).max(0.0))
    } else {
        ICON_SIZE.min(vertical)
    };
    let x = if icon_only {
        bounds.origin.x + (bounds.size.width - size) * 0.5
    } else {
        bounds.origin.x + padding.left
    };
    LogicalRect::from_xywh(
        x,
        bounds.origin.y + (bounds.size.height - size) * 0.5,
        size,
        size,
    )
}

fn paint_icon(
    painter: &mut Painter,
    icon: &Icon,
    bounds: LogicalRect,
    color: Color,
) -> Result<(), UiError> {
    let scale =
        (bounds.size.width / icon.view_box.width).min(bounds.size.height / icon.view_box.height);
    let offset = Vec2::new(
        bounds.origin.x + (bounds.size.width - icon.view_box.width * scale) * 0.5,
        bounds.origin.y + (bounds.size.height - icon.view_box.height * scale) * 0.5,
    );
    painter
        .with_save(|painter| {
            painter.transform(Affine2::from_scale_angle_translation(
                Vec2::splat(scale),
                0.0,
                offset,
            ))?;
            painter.fill_path(icon.path(), FillRule::NonZero, Brush::Solid(color))
        })
        .map_err(ui_error)
}

fn ui_error(error: impl fmt::Display) -> UiError {
    UiError::from_message(error.to_string())
}

/// Small built-in icon set used by Astreon controls and examples.
pub mod icons {
    use super::*;

    fn polygon(points: &[(f32, f32)]) -> Icon {
        let mut verbs = Vec::with_capacity(points.len() + 2);
        verbs.push(PathVerb::MoveTo(Point::new(points[0].0, points[0].1)));
        verbs.extend(
            points[1..]
                .iter()
                .map(|&(x, y)| PathVerb::LineTo(Point::new(x, y))),
        );
        verbs.push(PathVerb::Close);
        Icon::from_verbs(Size::new(24.0, 24.0), verbs).expect("built-in icon is valid")
    }

    /// Check mark.
    pub fn check() -> Icon {
        polygon(&[
            (3.0, 12.0),
            (6.0, 9.0),
            (10.0, 13.0),
            (18.0, 5.0),
            (21.0, 8.0),
            (10.0, 19.0),
        ])
    }

    /// Downward chevron.
    pub fn chevron_down() -> Icon {
        polygon(&[
            (4.0, 9.5),
            (12.0, 17.5),
            (20.0, 9.5),
            (17.0, 6.5),
            (12.0, 11.5),
            (7.0, 6.5),
        ])
    }

    /// Rightward chevron.
    pub fn chevron_right() -> Icon {
        polygon(&[
            (9.5, 4.0),
            (17.5, 12.0),
            (9.5, 20.0),
            (6.5, 17.0),
            (11.5, 12.0),
            (6.5, 7.0),
        ])
    }

    /// Plus sign.
    pub fn add() -> Icon {
        polygon(&[
            (10.0, 3.0),
            (14.0, 3.0),
            (14.0, 10.0),
            (21.0, 10.0),
            (21.0, 14.0),
            (14.0, 14.0),
            (14.0, 21.0),
            (10.0, 21.0),
            (10.0, 14.0),
            (3.0, 14.0),
            (3.0, 10.0),
            (10.0, 10.0),
        ])
    }

    /// Minus sign.
    pub fn remove() -> Icon {
        polygon(&[(3.0, 10.0), (21.0, 10.0), (21.0, 14.0), (3.0, 14.0)])
    }

    /// Close symbol.
    pub fn close() -> Icon {
        polygon(&[
            (5.0, 3.0),
            (12.0, 10.0),
            (19.0, 3.0),
            (21.0, 5.0),
            (14.0, 12.0),
            (21.0, 19.0),
            (19.0, 21.0),
            (12.0, 14.0),
            (5.0, 21.0),
            (3.0, 19.0),
            (10.0, 12.0),
            (3.0, 5.0),
        ])
    }

    /// Magnifying glass.
    pub fn search() -> Icon {
        polygon(&[
            (3.0, 3.0),
            (14.0, 3.0),
            (18.0, 7.0),
            (18.0, 13.0),
            (22.0, 17.0),
            (18.0, 21.0),
            (14.0, 17.0),
            (7.0, 17.0),
            (3.0, 13.0),
        ])
    }

    /// Crosshair reticle used for element picking.
    pub fn crosshair() -> Icon {
        let ring_outer = [
            (18.0, 12.0),
            (16.2, 16.2),
            (12.0, 18.0),
            (7.8, 16.2),
            (6.0, 12.0),
            (7.8, 7.8),
            (12.0, 6.0),
            (16.2, 7.8),
        ];
        let ring_inner = [
            (15.5, 12.0),
            (14.5, 14.5),
            (12.0, 15.5),
            (9.5, 14.5),
            (8.5, 12.0),
            (9.5, 9.5),
            (12.0, 8.5),
            (14.5, 9.5),
        ];
        let ticks = [
            [(11.0, 1.0), (13.0, 1.0), (13.0, 4.5), (11.0, 4.5)],
            [(11.0, 19.5), (13.0, 19.5), (13.0, 23.0), (11.0, 23.0)],
            [(1.0, 11.0), (4.5, 11.0), (4.5, 13.0), (1.0, 13.0)],
            [(19.5, 11.0), (23.0, 11.0), (23.0, 13.0), (19.5, 13.0)],
        ];
        let mut verbs = Vec::new();
        for subpath in [&ring_outer[..], &ring_inner[..]]
            .into_iter()
            .chain(ticks.iter().map(|tick| &tick[..]))
        {
            verbs.push(PathVerb::MoveTo(Point::new(subpath[0].0, subpath[0].1)));
            verbs.extend(
                subpath[1..]
                    .iter()
                    .map(|&(x, y)| PathVerb::LineTo(Point::new(x, y))),
            );
            verbs.push(PathVerb::Close);
        }
        Icon::from_verbs(Size::new(24.0, 24.0), verbs).expect("built-in icon is valid")
    }

    /// Window frame with a filled side panel, used for dock placement.
    pub fn dock() -> Icon {
        let frame = [
            // Top, bottom, left, and right 2px frame bars.
            [(3.0, 4.0), (21.0, 4.0), (21.0, 6.0), (3.0, 6.0)],
            [(3.0, 18.0), (21.0, 18.0), (21.0, 20.0), (3.0, 20.0)],
            [(3.0, 6.0), (5.0, 6.0), (5.0, 18.0), (3.0, 18.0)],
            [(19.0, 6.0), (21.0, 6.0), (21.0, 18.0), (19.0, 18.0)],
            // Filled band marking the docked panel strip.
            [(14.0, 6.0), (19.0, 6.0), (19.0, 18.0), (14.0, 18.0)],
        ];
        let mut verbs = Vec::new();
        for subpath in &frame {
            verbs.push(PathVerb::MoveTo(Point::new(subpath[0].0, subpath[0].1)));
            verbs.extend(
                subpath[1..]
                    .iter()
                    .map(|&(x, y)| PathVerb::LineTo(Point::new(x, y))),
            );
            verbs.push(PathVerb::Close);
        }
        Icon::from_verbs(Size::new(24.0, 24.0), verbs).expect("built-in icon is valid")
    }

    /// Settings gear with a distinct center opening.
    pub fn settings() -> Icon {
        let outer = [
            (9.0, 1.0),
            (15.0, 1.0),
            (16.0, 4.0),
            (18.0, 5.0),
            (21.0, 4.0),
            (23.0, 9.0),
            (20.0, 11.0),
            (20.0, 13.0),
            (23.0, 15.0),
            (21.0, 20.0),
            (18.0, 19.0),
            (16.0, 20.0),
            (15.0, 23.0),
            (9.0, 23.0),
            (8.0, 20.0),
            (6.0, 19.0),
            (3.0, 20.0),
            (1.0, 15.0),
            (4.0, 13.0),
            (4.0, 11.0),
            (1.0, 9.0),
            (3.0, 4.0),
            (6.0, 5.0),
            (8.0, 4.0),
        ];
        let inner = [
            (12.0, 7.5),
            (8.8, 8.8),
            (7.5, 12.0),
            (8.8, 15.2),
            (12.0, 16.5),
            (15.2, 15.2),
            (16.5, 12.0),
            (15.2, 8.8),
        ];
        let mut verbs = Vec::with_capacity(outer.len() + inner.len() + 4);
        verbs.push(PathVerb::MoveTo(Point::new(outer[0].0, outer[0].1)));
        verbs.extend(
            outer[1..]
                .iter()
                .map(|&(x, y)| PathVerb::LineTo(Point::new(x, y))),
        );
        verbs.push(PathVerb::Close);
        verbs.push(PathVerb::MoveTo(Point::new(inner[0].0, inner[0].1)));
        verbs.extend(
            inner[1..]
                .iter()
                .map(|&(x, y)| PathVerb::LineTo(Point::new(x, y))),
        );
        verbs.push(PathVerb::Close);
        Icon::from_verbs(Size::new(24.0, 24.0), verbs).expect("built-in icon is valid")
    }

    /// Folder.
    pub fn folder() -> Icon {
        polygon(&[
            (2.0, 5.0),
            (10.0, 5.0),
            (12.0, 8.0),
            (22.0, 8.0),
            (22.0, 20.0),
            (2.0, 20.0),
        ])
    }

    /// Save/disk.
    pub fn save() -> Icon {
        polygon(&[
            (3.0, 2.0),
            (18.0, 2.0),
            (22.0, 6.0),
            (22.0, 22.0),
            (2.0, 22.0),
            (2.0, 2.0),
            (7.0, 2.0),
            (7.0, 9.0),
            (17.0, 9.0),
            (17.0, 2.0),
        ])
    }

    /// Undo arrow.
    pub fn undo() -> Icon {
        polygon(&[
            (2.0, 10.0),
            (9.0, 3.0),
            (9.0, 8.0),
            (15.0, 8.0),
            (21.0, 13.0),
            (21.0, 20.0),
            (17.0, 20.0),
            (17.0, 15.0),
            (14.0, 12.0),
            (9.0, 12.0),
            (9.0, 17.0),
        ])
    }

    /// Redo arrow.
    pub fn redo() -> Icon {
        polygon(&[
            (22.0, 10.0),
            (15.0, 3.0),
            (15.0, 8.0),
            (9.0, 8.0),
            (3.0, 13.0),
            (3.0, 20.0),
            (7.0, 20.0),
            (7.0, 15.0),
            (10.0, 12.0),
            (15.0, 12.0),
            (15.0, 17.0),
        ])
    }
}

#[cfg(test)]
mod tests {
    use astrelis_text::FontDatabase;
    use astrelis_ui_core::{SemanticAction, Ui};

    use super::*;

    #[test]
    fn rejects_invalid_view_boxes_and_empty_paths() {
        assert!(Icon::new(Size::new(0.0, 24.0), Path::builder().finish()).is_err());
        assert!(Icon::new(Size::new(24.0, 24.0), Path::builder().finish()).is_err());
    }

    #[test]
    fn icon_only_slot_is_centered_and_labelled_slot_hugs_the_left_padding() {
        let padding = astrelis_ui_core::Insets {
            left: 10.0,
            top: 6.0,
            right: 10.0,
            bottom: 6.0,
        };
        // Compact 24x24 square, e.g. a tree disclosure.
        let compact = icon_slot(LogicalRect::from_xywh(0.0, 0.0, 24.0, 24.0), padding, true);
        assert_eq!(compact.size.width, compact.size.height);
        assert_eq!(compact.origin.x + compact.size.width * 0.5, 12.0);
        assert_eq!(compact.origin.y + compact.size.height * 0.5, 12.0);
        // Default 28x28 square, e.g. a close button.
        let square = icon_slot(LogicalRect::from_xywh(100.0, 40.0, 28.0, 28.0), padding, true);
        assert_eq!(square.size.width, 16.0);
        assert_eq!(square.origin.x + square.size.width * 0.5, 114.0);
        assert_eq!(square.origin.y + square.size.height * 0.5, 54.0);
        // Labelled buttons keep the fixed left slot so text stays aligned.
        let labelled = icon_slot(LogicalRect::from_xywh(0.0, 0.0, 96.0, 28.0), padding, false);
        assert_eq!(labelled.origin.x, 10.0);
        assert_eq!(labelled.origin.y + labelled.size.height * 0.5, 14.0);
    }

    #[test]
    fn chevron_visual_bounds_are_centered_in_the_view_box() {
        for icon in [icons::chevron_down(), icons::chevron_right()] {
            let bounds = icon.path().bounds().expect("built-in icon has bounds");
            assert_eq!(bounds.origin.x + bounds.size.width * 0.5, 12.0);
            assert_eq!(bounds.origin.y + bounds.size.height * 0.5, 12.0);
        }
    }

    #[test]
    fn icon_button_exposes_and_performs_semantic_activation() {
        let mut ui = Ui::new(FontDatabase::default(), Theme::default());
        let button = ui
            .add_widget(ui.root(), IconButton::icon_only(icons::save(), "Save", 7))
            .unwrap();
        ui.set_viewport(Size::new(200.0, 100.0), 1.0);
        let semantics = ui.semantic_tree().unwrap();
        assert!(
            semantics
                .children
                .iter()
                .any(|node| { node.role == SemanticRole::Button && node.label == "Save" })
        );
        ui.perform_semantic_action(button.id(), SemanticAction::Activate)
            .unwrap();
        assert_eq!(ui.drain_messages().collect::<Vec<_>>(), vec![7]);
    }

    #[test]
    fn disabling_a_focused_command_button_clears_its_focus_ring() {
        let mut ui = Ui::new(FontDatabase::default(), Theme::default());
        let button = ui
            .add_widget(ui.root(), CommandButton::new("Undo", 7))
            .unwrap();
        ui.focus(button).unwrap();
        assert!(ui.widget(button).unwrap().focused);

        ui.update_widget(button, |button| button.sync("Undo", false, false))
            .unwrap();
        ui.set_enabled(button, false).unwrap();

        assert!(!ui.widget(button).unwrap().focused);
        assert!(!ui.is_focused(button).unwrap());
    }
}
