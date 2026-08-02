//! Composite leaf controls.
//!
//! Everything here builds on the view kinds in [`crate::views`] and adds no new
//! `ViewNode` implementation - a `radio_group` is a keyed `column` of buttons,
//! nothing more. The distinction matters when reading the crate: `views/` is
//! where the protocol is implemented, `controls/` is where it is used.
//!
//! Application *surfaces* live in [`crate::surfaces`] instead, because they
//! carry policy - focus scope, escape dismissal, fuzzy matching - that a leaf
//! control does not.

mod choice;
mod combo;
mod numeric;

pub use choice::*;
pub use combo::*;
pub use numeric::*;
