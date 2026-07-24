//! Component-native desktop UI framework.
//!
//! RXUI's public authoring API is the reconciled component/view system. The
//! retained Astrelis core remains available through [`core`] for specialized
//! elements, but the former retained widget and application facades have been
//! removed.

#![warn(missing_docs)]

pub use rxui_next::*;

/// Common component, view, catalog, host, and service types.
pub mod prelude {
    pub use rxui_next::*;
}
