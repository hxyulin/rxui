//! Controlled form validation.
//!
//! Split along the only axis that matters for testing it: the model half is
//! pure application-owned state with no view vocabulary at all, so its state
//! machine is unit-testable without mounting anything, and the view half
//! renders it.

mod model;
mod view;

pub use model::*;
pub use view::*;
