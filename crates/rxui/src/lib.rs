//! Component-native desktop UI framework.
//!
//! RXUI's public authoring API is the reconciled component/view system. The
//! retained Astrelis core remains available through [`core`] for specialized
//! elements, but the former retained widget and application facades have been
//! removed.

#![warn(missing_docs)]

pub use rxui_controls::*;
pub use rxui_core::*;
pub use rxui_native::*;
pub use rxui_workbench::*;

/// Common component, view, catalog, host, and service types.
pub mod prelude {
    pub use rxui_controls::*;
    pub use rxui_core::*;
    pub use rxui_native::*;
    pub use rxui_workbench::*;
}
