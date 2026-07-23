//! Typed semantic theme tokens.

use astrelis_core::color::Color;

/// Semantic color role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorRole {
    /// Window background.
    Background,
    /// Raised panel surface.
    Surface,
    /// Primary text.
    Text,
    /// Muted text and separators.
    Muted,
    /// Application accent.
    Accent,
    /// Destructive action.
    Danger,
}

/// Semantic spacing token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Space {
    /// No spacing.
    None,
    /// Four logical pixels.
    Xs,
    /// Eight logical pixels.
    Sm,
    /// Twelve logical pixels.
    Md,
    /// Sixteen logical pixels.
    Lg,
    /// Twenty-four logical pixels.
    Xl,
}

/// Semantic button presentation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ButtonVariant {
    /// Standard surface button.
    #[default]
    Standard,
    /// Accent-filled primary action.
    Primary,
    /// Low-emphasis toolbar action.
    Quiet,
    /// Destructive action.
    Destructive,
}

/// Typed design-system tokens used by experimental views.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Monotonic identity used by style caches.
    pub revision: u64,
    /// Window background.
    pub background: Color,
    /// Raised surface.
    pub surface: Color,
    /// Main foreground.
    pub text: Color,
    /// Secondary foreground.
    pub muted: Color,
    /// Accent.
    pub accent: Color,
    /// Destructive accent.
    pub danger: Color,
}

impl Theme {
    /// Dark editor-oriented defaults.
    pub fn dark() -> Self {
        Self {
            revision: 1,
            background: Color::from_hex(0x16181d),
            surface: Color::from_hex(0x23262e),
            text: Color::from_hex(0xe7eaf0),
            muted: Color::from_hex(0x7f8798),
            accent: Color::from_hex(0x4c8dff),
            danger: Color::from_hex(0xe05b65),
        }
    }

    /// Resolves a semantic color role.
    pub const fn color(&self, role: ColorRole) -> Color {
        match role {
            ColorRole::Background => self.background,
            ColorRole::Surface => self.surface,
            ColorRole::Text => self.text,
            ColorRole::Muted => self.muted,
            ColorRole::Accent => self.accent,
            ColorRole::Danger => self.danger,
        }
    }

    /// Resolves a spacing token.
    pub const fn space(&self, space: Space) -> f32 {
        match space {
            Space::None => 0.0,
            Space::Xs => 4.0,
            Space::Sm => 8.0,
            Space::Md => 12.0,
            Space::Lg => 16.0,
            Space::Xl => 24.0,
        }
    }

    /// Resolves button background and pressed colors.
    pub const fn button(&self, variant: ButtonVariant) -> (Color, Color) {
        match variant {
            ButtonVariant::Standard => (self.surface, self.muted),
            ButtonVariant::Primary => (self.accent, self.text),
            ButtonVariant::Quiet => (self.background, self.surface),
            ButtonVariant::Destructive => (self.danger, self.text),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
