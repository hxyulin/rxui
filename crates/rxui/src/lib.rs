//! Declarative RXUI's headless state foundation.
//!
//! Persistent typed entities mutate through synchronous context/closure scopes.
//! Mount evaluation tracks model dependencies and binds listeners to current
//! state. Updates invalidate dependent mounts automatically; effects are coalesced
//! until an explicit flush. The runtime performs no native/GPU work.
//!
//! This first milestone implements state, access, mounts and callbacks. Element
//! builders, Taffy layout, widgets and the native Application host are next steps.
//! Integration features expose the pinned dependencies, not an implemented host.
//!
//! ```
//! use rxui::Runtime;
//! let mut runtime = Runtime::new();
//! let (counter, mount) = runtime.update(|cx| {
//!     let counter = cx.new(|_| 0_u32);
//!     let mount = cx.mount(&counter).unwrap();
//!     (counter, mount)
//! });
//! let increment = runtime.evaluate(&mount, |_, cx| {
//!     cx.listener(|count, _: &(), _| *count += 1)
//! }).unwrap();
//! assert!(!runtime.is_dirty(&mount).unwrap());
//! runtime.update(|cx| increment.dispatch(&(), cx)).unwrap();
//! assert!(runtime.is_dirty(&mount).unwrap());
//! runtime.flush().unwrap();
//! assert_eq!(runtime.evaluate(&mount, |count, _| *count).unwrap(), 1);
//! ```
//!
//! Read access cannot overlap a subsequent mutable context operation:
//!
//! ```compile_fail
//! use rxui::Runtime;
//! let mut runtime = Runtime::new();
//! let count = runtime.update(|cx| cx.new(|_| 0_u32));
//! runtime.update(|cx| {
//!     let read = count.read(cx);
//!     count.update(cx, |count, _| *count += 1);
//!     println!("{}", *read);
//! });
//! ```
//!
//! Evaluation contexts do not provide mutation capability:
//!
//! ```compile_fail
//! use rxui::Runtime;
//! let mut runtime = Runtime::new();
//! let (count, mount) = runtime.update(|cx| {
//!     let count = cx.new(|_| 0_u32);
//!     let mount = cx.mount(&count).unwrap();
//!     (count, mount)
//! });
//! runtime.evaluate(&mount, |_, cx| count.update(cx, |count, _| *count += 1));
//! ```
//!
//! A listener cannot capture the borrowed owner from evaluation:
//!
//! ```compile_fail
//! use rxui::Runtime;
//! let mut runtime = Runtime::new();
//! let mount = runtime.update(|cx| {
//!     let count = cx.new(|_| 0_u32);
//!     cx.mount(&count).unwrap()
//! });
//! runtime.evaluate(&mount, |borrowed, cx| {
//!     cx.listener(move |state, _: &(), _| *state = *borrowed)
//! });
//! ```

mod context;
mod entity;
mod error;
mod id;
mod listener;
mod runtime;

pub use context::{AppContext, Context, ReadContext, ViewContext};
pub use entity::{Entity, Read, WeakEntity};
pub use error::{AccessError, EffectCycle};
pub use id::{EntityId, MountId};
pub use listener::{Dispatch, Listener};
pub use runtime::{Mount, Runtime, Subscription};

/// Pinned graphics integration dependency; UI painting is a later milestone.
#[cfg(feature = "rendering")]
pub use astrelis;
/// Pinned native lifecycle dependency; Application hosting is a later milestone.
#[cfg(feature = "native")]
pub use astrelis_winit;
/// Selected layout dependency; element layout is a later milestone.
#[cfg(feature = "layout")]
pub use taffy;

#[cfg(test)]
mod tests;
