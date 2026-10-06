//! Declarative RXUI's headless state foundation.
//!
//! Persistent typed entities mutate through synchronous context/closure scopes.
//! Mount evaluation tracks model dependencies and binds listeners to current
//! state. Updates invalidate dependent mounts automatically; effects are coalesced
//! until an explicit flush. The runtime performs no native/GPU work.
//!
//! The default layout feature adds owned element builders, keyed reconciliation,
//! Taffy flex layout, retained theme resolution, inherited text styling, scrolling,
//! clipping and basic button routing. Dark/light presets and paint-only control states
//! keep application styling separate from drawing. Controlled
//! single-line inputs retain selection and IME preedit independently of
//! application-owned values. The optional
//! tasks feature delivers owned background results in fresh UI updates; native
//! adds Application hosting over astrelis-winit with lazy AccessKit publication.
//! Portable semantics are part of layout; the optional accessibility feature adds
//! AccessKit translation for custom hosts. Multiline/undo editing and virtualization
//! remain following milestones.
//!
//! Declarative components own their descriptions and bind live-state listeners:
//!
//! ```
//! # #[cfg(feature = "layout")]
//! # {
//! use rxui::prelude::*;
//! struct Counter { count: u32 }
//! impl View for Counter {
//!     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
//!         column().padding(24.).gap(12.)
//!             .child(label(format!("Count: {}", self.count)))
//!             .child(button("Increase").key("increase")
//!                 .on_click(cx.listener(|this, _, _| this.count += 1)))
//!     }
//! }
//! let mut runtime = Runtime::new();
//! let counter = runtime.update(|cx| cx.new(|_| Counter { count: 0 }));
//! let ui = Ui::new(&mut runtime, counter).unwrap();
//! assert!(!ui.is_prepared()); // A host supplies TextMeasure and a viewport next.
//! # }
//! ```
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

#[cfg(feature = "accessibility")]
mod accessibility;
mod context;
#[cfg(feature = "layout")]
mod editing;
#[cfg(feature = "layout")]
mod element;
mod entity;
mod error;
mod id;
mod listener;
#[cfg(feature = "native")]
mod native;
#[cfg(feature = "rendering")]
mod painting;
mod runtime;
#[cfg(feature = "layout")]
mod semantics;
#[cfg(feature = "tasks")]
mod tasks;
#[cfg(feature = "layout")]
mod theme;
#[cfg(feature = "layout")]
mod ui;

#[cfg(feature = "accessibility")]
pub use accessibility::{AccessKitStats, AccessKitTree};
#[cfg(feature = "accessibility")]
pub use accesskit;
/// OS close-request policy used by Application's lifecycle hook.
#[cfg(feature = "native")]
pub use astrelis_winit::CloseResponse;
pub use context::{AppContext, Context, ReadContext, ViewContext};
#[cfg(feature = "layout")]
pub use editing::{
    TextAffinity, TextChangeEvent, TextInputEvent, TextInputInfo, TextMovement, TextPosition,
    TextSelection, TextSubmitEvent,
};
#[cfg(feature = "layout")]
pub use element::{
    ClickEvent, Color, Element, IntoElement, Key, ScrollAxes, View, button, column, label, row,
    text_input,
};
pub use entity::{Entity, Read, WeakEntity};
pub use error::{AccessError, EffectCycle};
pub use id::{EntityId, MountId};
pub use listener::{Dispatch, Listener};
#[cfg(feature = "native")]
pub use native::{Application, ApplicationError, WindowHandle, WindowId, WindowOptions};
#[cfg(feature = "rendering")]
pub use painting::UiPainter;
pub use runtime::{Mount, Runtime, Subscription};
#[cfg(feature = "layout")]
pub use semantics::{SemanticAction, SemanticNode, SemanticRole};
#[cfg(feature = "tasks")]
pub use tasks::{
    BackgroundFuture, BlockingJob, SpawnError, Task, TaskError, TaskExecutor, TaskResult,
};
#[cfg(all(feature = "tasks", not(target_arch = "wasm32")))]
pub use tasks::{ThreadPoolExecutor, sleep};
#[cfg(feature = "layout")]
pub use theme::{
    PaintStyle, ResolvedPaint, StyleColor, Theme, ThemeColor, ThemeColors, ThemeMetrics, rgb8,
    rgba8,
};
#[cfg(feature = "layout")]
pub use ui::{
    Bounds, ElementId, ElementInfo, ElementType, PointerEvent, TextMeasure, TextRequest, TextWidth,
    Ui, UiError, UiStats,
};

/// Common declarative application imports.
#[cfg(feature = "layout")]
pub mod prelude {
    #[cfg(feature = "native")]
    pub use crate::{Application, ApplicationError, CloseResponse, WindowHandle, WindowOptions};
    pub use crate::{
        ClickEvent, Context, Entity, IntoElement, Listener, PaintStyle, Runtime, SemanticRole,
        TextChangeEvent, TextSubmitEvent, Theme, ThemeColor, Ui, View, ViewContext, button, column,
        label, rgb8, rgba8, row, text_input,
    };
    #[cfg(feature = "tasks")]
    pub use crate::{Task, TaskError, TaskResult};
}

/// Pinned graphics integration dependency used by UiPainter.
#[cfg(feature = "rendering")]
pub use astrelis;
/// Pinned native lifecycle dependency used by Application and custom hosts.
#[cfg(feature = "native")]
pub use astrelis_winit;
/// Selected flex layout solver and style customization types.
#[cfg(feature = "layout")]
pub use taffy;

#[cfg(all(test, feature = "tasks"))]
mod task_tests;
#[cfg(test)]
mod tests;
#[cfg(all(test, feature = "layout"))]
mod ui_tests;

#[cfg(all(test, feature = "layout"))]
mod editing_tests;

#[cfg(all(test, feature = "layout"))]
mod semantic_tests;

#[cfg(all(test, feature = "layout"))]
mod theme_tests;
