//! Batteries-included retained-mode desktop UI framework.

#![warn(missing_docs)]

/// Low-level retained UI escape hatch.
pub use astrelis_ui as ui;
/// Application hosting and shared commands.
pub use astreon_app as app;
#[cfg(feature = "editor")]
/// Editor workspace components.
pub use astreon_editor as editor;
#[cfg(feature = "testing")]
/// Deterministic testing helpers.
pub use astreon_testing as testing;
/// Application-oriented widgets.
pub use astreon_widgets as widgets;

/// Common types for ordinary Astreon application code.
pub mod prelude {
    pub use astrelis_ui::prelude::*;
    pub use astreon_app::{
        Command, CommandId, CommandRegistry, GraphicsContext, Shortcut, WindowHost,
        WindowHostOptions,
    };
    pub use astreon_widgets::{ThemePreference, ThemeSet};
}
