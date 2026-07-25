//! Typed presentation options resolved against a [`crate::Theme`].

use astrelis_core::geometry::LogicalSize;

use crate::{ButtonVariant, ColorRole, Space};
/// Explicit sizing and flex-growth options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameStyle {
    /// Optional preferred width.
    pub width: Option<f32>,
    /// Optional preferred height.
    pub height: Option<f32>,
    /// Minimum size.
    pub min: LogicalSize,
    /// Optional maximum size.
    pub max: Option<LogicalSize>,
    /// Relative share of remaining flex space.
    pub grow: f32,
}

impl FrameStyle {
    /// Creates an unconstrained, non-growing frame.
    pub const fn new() -> Self {
        Self {
            width: None,
            height: None,
            min: LogicalSize::ZERO,
            max: None,
            grow: 0.0,
        }
    }

    /// Selects preferred width.
    pub const fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Selects preferred height.
    pub const fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Selects relative main-axis growth.
    pub const fn grow(mut self, grow: f32) -> Self {
        self.grow = grow;
        self
    }

    /// Selects minimum size.
    pub const fn min(mut self, min: LogicalSize) -> Self {
        self.min = min;
        self
    }

    /// Selects maximum size.
    pub const fn max(mut self, max: LogicalSize) -> Self {
        self.max = Some(max);
        self
    }
}

impl Default for FrameStyle {
    fn default() -> Self {
        Self::new()
    }
}
/// Typed label presentation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabelStyle {
    /// Logical font size.
    pub font_size: f32,
    /// Semantic text color.
    pub role: ColorRole,
    /// Optional preferred width.
    pub width: Option<f32>,
}

impl LabelStyle {
    /// Creates standard body-label presentation.
    pub const fn standard() -> Self {
        Self {
            font_size: 14.0,
            role: ColorRole::Text,
            width: None,
        }
    }

    /// Selects the logical font size.
    pub const fn font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    /// Selects a semantic text color.
    pub const fn role(mut self, role: ColorRole) -> Self {
        self.role = role;
        self
    }

    /// Selects an optional preferred width.
    pub const fn width(mut self, width: Option<f32>) -> Self {
        self.width = width;
        self
    }
}

impl Default for LabelStyle {
    fn default() -> Self {
        Self::standard()
    }
}
/// Typed presentation options for a button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonStyle {
    /// Semantic color treatment.
    pub variant: ButtonVariant,
    /// Preferred logical size.
    pub size: LogicalSize,
}

impl ButtonStyle {
    /// Creates standard button presentation.
    pub const fn standard() -> Self {
        Self {
            variant: ButtonVariant::Standard,
            size: LogicalSize::new(0.0, 30.0),
        }
    }

    /// Selects a semantic variant.
    pub const fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Selects the preferred logical size.
    pub const fn size(mut self, size: LogicalSize) -> Self {
        self.size = size;
        self
    }
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self::standard()
    }
}
/// Typed presentation options for row and column containers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContainerStyle {
    /// Space between children.
    pub gap: Space,
    /// Uniform inset around children.
    pub padding: Space,
    /// Optional semantic background fill.
    pub background: Option<ColorRole>,
}

impl Default for ContainerStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerStyle {
    /// Creates the default compact container style.
    pub const fn new() -> Self {
        Self {
            gap: Space::Sm,
            padding: Space::None,
            background: None,
        }
    }

    /// Selects semantic child spacing.
    pub const fn gap(mut self, gap: Space) -> Self {
        self.gap = gap;
        self
    }

    /// Selects uniform semantic padding.
    pub const fn padding(mut self, padding: Space) -> Self {
        self.padding = padding;
        self
    }

    /// Selects a semantic background role.
    pub const fn background(mut self, background: ColorRole) -> Self {
        self.background = Some(background);
        self
    }
}
/// Typed presentation options for an overlay stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackStyle {
    /// Uniform inset around overlaid children.
    pub padding: Space,
    /// Optional semantic background fill.
    pub background: Option<ColorRole>,
}

impl Default for StackStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl StackStyle {
    /// Creates an undecorated stack.
    pub const fn new() -> Self {
        Self {
            padding: Space::None,
            background: None,
        }
    }

    /// Selects uniform semantic padding.
    pub const fn padding(mut self, padding: Space) -> Self {
        self.padding = padding;
        self
    }

    /// Selects a semantic background role.
    pub const fn background(mut self, background: ColorRole) -> Self {
        self.background = Some(background);
        self
    }
}
