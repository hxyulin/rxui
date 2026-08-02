//! Application surfaces.
//!
//! These compose the same view kinds as [`crate::controls`] but additionally
//! own *policy*: a dialog installs a focus scope and dismisses on escape, a
//! command palette matches a query and clamps a selection, a toolbar assigns
//! canonical identities to its decorations. That policy is why they are not
//! filed as leaf controls.

mod dialog;
mod palette;
mod toast;
mod toolbar;

pub use dialog::*;
pub use palette::*;
pub use toast::*;
pub use toolbar::*;
