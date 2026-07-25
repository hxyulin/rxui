//! The view kinds `rxui-core` ships, each with the state that maintains it.
//!
//! Every kind here is implemented against the public protocol in
//! [`crate::view`] with no privileged access, which is what makes that protocol
//! demonstrably sufficient for a view kind defined outside this crate.

mod component;
mod controls;
mod layout;
mod modifier;
mod retained;
mod text;

pub use component::component;
pub(crate) use controls::icon_button_view;
pub use controls::{button, button_with, checkbox, slider, slider_with_step, text_field};
pub use layout::{
    column, column_with, flex, panel, row, row_with, scroll, scroll_at, spacer, split_pane, stack,
    stack_with,
};
pub use retained::{RetainedSpec, retained};
pub use text::{label, label_with_style, label_with_width};
