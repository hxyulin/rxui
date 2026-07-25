//! Lightweight view values, the open view protocol, and keyed reconciliation.
//!
//! A [`View`] is a description, not mounted state. Building one allocates a
//! small tree of [`ViewNode`] values; mounting it walks that tree once and
//! produces a parallel tree of [`Mounted`] nodes that own retained identity.
//! Every later pass reconciles a fresh description against the mounted tree.
//!
//! The protocol is open: the twenty view kinds [`crate::views`] ships are
//! written against exactly the surface a third-party crate gets. See
//! [`ViewNode`] for the contract and `docs/view-protocol.md` for the guide.
//!
//! This module holds the protocol itself. The view kinds live in
//! [`crate::views`], each co-located with the mounted state it maintains.

mod context;
mod host;
mod key;
mod protocol;
mod reconcile;
mod style;

pub use context::ViewContext;
pub use host::ViewHost;
pub use key::{ViewKey, ViewKind};
pub(crate) use protocol::{
    ActionCell, MapCell, container_mounted_state, leaf_mounted_state, wrapper_mounted_state,
};
pub use protocol::{
    ActionEmitter, AnyView, DynamicViews, IntoChildren, Mounted, MountedState, RebuildContext,
    RouteContext, View, ViewNode, views,
};
pub use reconcile::MountedChildren;
pub use style::{ButtonStyle, ContainerStyle, FrameStyle, LabelStyle, StackStyle};
