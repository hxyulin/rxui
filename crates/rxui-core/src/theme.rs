//! Minimal semantic theme tokens and revision identity.

use astrelis_core::color::Color;

/// Application-wide visual tokens.
///
/// `revision` is the invalidation identity. Applications must advance it when
/// changing tokens so every mounted entity gets one render at the new theme.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Monotonic identity used by entity render boundaries.
    pub revision: u64,
    /// Window background.
    pub background: Color,
    /// Raised surface.
    pub surface: Color,
    /// Primary text.
    pub text: Color,
    /// Secondary text.
    pub muted: Color,
    /// Application accent.
    pub accent: Color,
    /// Destructive accent.
    pub danger: Color,
}

impl Theme {
    /// Returns the dark editor-oriented default theme.
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
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
