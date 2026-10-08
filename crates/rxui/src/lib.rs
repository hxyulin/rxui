//! Declarative RXUI's headless state foundation.
//!
//! Persistent typed entities mutate through synchronous context/closure scopes.
//! Mount evaluation tracks model dependencies and binds listeners to current
//! state. Updates invalidate dependent mounts automatically; effects are coalesced
//! until an explicit flush. The runtime performs no native/GPU work.
//!
//! The default layout feature adds owned element builders, keyed reconciliation,
//! Taffy flex layout, retained theme resolution, inherited text styling, scrolling,
//! clipping, routed pointer/keyboard listeners, capture, scroll handles/scrollbars
//! and controlled split panes. Focus groups, placement-local focus handles and
//! controlled tabs preserve panel state with explicit mounting policies. An
//! application-owned docking tree composes them with checked topology edits.
//! Viewport popovers and controlled modal dialogs add placement, dismissal and
//! focus restoration. Typed command actions share availability and callbacks
//! across scoped shortcuts, buttons and popup menu items, with explicit application
//! fallback registrations. Overlay semantics escape ancestor clipping.
//! Dark/light presets and paint-only control states keep application styling
//! separate from drawing. Controlled
//! single-line inputs retain selection, bounded undo/redo and IME preedit independently of
//! application-owned values. The optional
//! tasks feature delivers owned background results in fresh UI updates; native
//! adds Application hosting over astrelis-winit with lazy AccessKit publication.
//! Optional native-dialogs adds parent-bound file pickers and message dialogs;
//! desktop-services adds asynchronous URL/file launches and file-manager reveal.
//! Native clipboard text access shares the host's persistent clipboard owner.
//! Native window geometry snapshots support application-owned persistence and
//! monitor-aware restoration; file-drop hooks deliver window-level native paths.
//! Portable semantics are part of layout; the optional accessibility feature adds
//! AccessKit translation for custom hosts. Fixed-height virtual lists describe only
//! viewport rows plus overscan; shared windows retain independent scroll offsets.
//! Multiline editing and variable-height virtualization remain following milestones.
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
#[cfg(feature = "layout")]
mod commands;
mod context;
#[cfg(feature = "layout")]
mod controls;
#[cfg(feature = "layout")]
mod custom;
#[cfg(feature = "layout")]
mod dock;
#[cfg(feature = "layout")]
mod dock_view;
#[cfg(feature = "layout")]
mod editing;
#[cfg(feature = "layout")]
mod element;
mod entity;
mod error;
mod id;
#[cfg(feature = "layout")]
mod image;
#[cfg(feature = "layout")]
mod input;
mod listener;
#[cfg(feature = "native")]
mod native;
#[cfg(feature = "native-menus")]
mod native_menus;
#[cfg(feature = "layout")]
mod overlay;
#[cfg(feature = "rendering")]
mod painting;
mod runtime;
#[cfg(feature = "layout")]
mod scrolling;
#[cfg(feature = "layout")]
mod semantics;
#[cfg(feature = "layout")]
mod tabs;
#[cfg(feature = "tasks")]
mod tasks;
#[cfg(feature = "layout")]
mod theme;
#[cfg(feature = "layout")]
mod typography;
#[cfg(feature = "layout")]
mod ui;
#[cfg(feature = "layout")]
mod virtual_list;

#[cfg(feature = "accessibility")]
pub use accessibility::{AccessKitStats, AccessKitTree};
#[cfg(feature = "accessibility")]
pub use accesskit;
/// OS close-request policy used by Application's lifecycle hook.
#[cfg(feature = "native")]
pub use astrelis_winit::CloseResponse;
#[cfg(feature = "layout")]
pub use commands::{
    Command, CommandAction, CommandId, CommandInfo, CommandRegistration, CommandStatus, Shortcut,
    standard_commands,
};
pub use context::{AppContext, Context, ReadContext, ViewContext};
#[cfg(feature = "layout")]
pub use controls::{
    Axis, RangeInfo, ResizeEvent, ResizePhase, ScrollArea, Split, SplitPosition, scroll_area,
    scrollbar, split_column, split_row,
};
#[cfg(feature = "rendering")]
pub use custom::CustomPrepare;
#[cfg(feature = "layout")]
pub use custom::{CustomElement, CustomMeasure, custom};
#[cfg(feature = "layout")]
pub use dock::{
    DockDropTarget, DockError, DockEvent, DockNode, DockNodeId, DockSide, DockSplit, DockTabs,
    DockTree,
};
#[cfg(feature = "layout")]
pub use dock_view::{Dock, DockContextEvent, DockPanel, dock, dock_panel};
#[cfg(feature = "layout")]
pub use editing::{
    TextAffinity, TextChangeEvent, TextInputEvent, TextInputInfo, TextMovement, TextPosition,
    TextSelection, TextSubmitEvent,
};
#[cfg(feature = "layout")]
pub use element::{
    ButtonVariant, ClickEvent, Color, Element, IntoElement, Key, PointerEvents, ScrollAxes, View,
    button, column, image, label, row, stack, text_input,
};
pub use entity::{Entity, Read, WeakEntity};
pub use error::{AccessError, EffectCycle};
pub use id::{EntityId, MountId};
#[cfg(feature = "layout")]
pub use image::{Image, ImageAlpha, ImageFilter, ImageFit, ImageId, ImageInfo};
#[cfg(feature = "layout")]
pub use input::{
    Cursor, EventPhase, HoverEvent, InputResult, KeyEvent, KeyInput, KeyboardKey, Modifiers,
    PointerButton, PointerButtons, PointerCancelReason, PointerInput, WheelInput,
};
pub use listener::{Dispatch, Listener};
#[cfg(feature = "native")]
pub use native::{
    Application, ApplicationError, GraphicsPrepareContext, WindowHandle, WindowId, WindowOptions,
};
#[cfg(feature = "native")]
pub use native::{FileDropEvent, WindowGeometry};
#[cfg(feature = "native-menus")]
pub use native_menus::{NativeMenu, NativeMenuBar, NativeMenuRole};
#[cfg(feature = "layout")]
pub use overlay::{
    AnchorHandle, DismissEvent, DismissReason, Overlay, OverlayAnchor, PopoverPlacement, menu,
    menu_item, modal, popover,
};
#[cfg(feature = "rendering")]
pub use painting::{ComposedUi, ImageStats, LayerStats, UiPainter};
pub use runtime::{Mount, Runtime, Subscription};
#[cfg(feature = "layout")]
pub use scrolling::{ScrollError, ScrollHandle, ScrollPlacement, ScrollState};
#[cfg(feature = "layout")]
pub use semantics::{SemanticAction, SemanticNode, SemanticRole};
#[cfg(feature = "layout")]
pub use tabs::{
    Tab, TabActivation, TabCloseEvent, TabContentPolicy, TabSelectEvent, Tabs, tab, tabs,
};
#[cfg(feature = "tasks")]
pub use tasks::{
    BackgroundFuture, BlockingJob, SpawnError, Task, TaskError, TaskExecutor, TaskResult,
};
#[cfg(all(feature = "tasks", not(target_arch = "wasm32")))]
pub use tasks::{ThreadPoolExecutor, sleep};
#[cfg(feature = "layout")]
pub use theme::{
    BoxShadow, PaintStyle, ResolvedPaint, ResolvedShadow, StyleColor, Theme, ThemeColor,
    ThemeColors, ThemeMetrics, rgb8, rgba8,
};
#[cfg(feature = "layout")]
pub use typography::{FontFamily, FontStyle, FontWeight, TextAlign, TextStyle};
#[cfg(feature = "layout")]
pub use ui::dock_dispatch::{DockDragInfo, DockDropPreview};
#[cfg(feature = "layout")]
pub use ui::focus::{FocusError, FocusHandle, FocusPlacement, FocusScope};
#[cfg(feature = "layout")]
pub use ui::{
    Bounds, ElementId, ElementInfo, ElementType, PointerEvent, RoundedClip, TextMeasure,
    TextRequest, TextWidth, Ui, UiError, UiStats,
};
#[cfg(feature = "layout")]
pub use virtual_list::{VirtualList, virtual_list};

#[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
pub use native::{
    DialogError, DialogResult, DialogTask, FileDialog, MessageButtons, MessageDialog, MessageLevel,
    MessageResponse,
};

#[cfg(all(feature = "desktop-services", not(target_arch = "wasm32")))]
pub use native::{DesktopError, DesktopResult};

/// Common declarative application imports.
#[cfg(feature = "layout")]
pub mod prelude {
    pub use crate::{
        AnchorHandle, Command, CommandAction, CommandId, CommandInfo, CommandRegistration,
        CommandStatus, DismissEvent, DismissReason, Overlay, OverlayAnchor, PopoverPlacement,
        Shortcut, menu, menu_item, modal, popover, standard_commands,
    };
    #[cfg(feature = "native")]
    pub use crate::{
        Application, ApplicationError, CloseResponse, FileDropEvent, WindowGeometry, WindowHandle,
        WindowOptions,
    };
    pub use crate::{
        Axis, RangeInfo, ResizeEvent, ResizePhase, ScrollArea, ScrollAxes, ScrollError,
        ScrollHandle, ScrollPlacement, ScrollState, Split, SplitPosition, VirtualList, scroll_area,
        scrollbar, split_column, split_row, virtual_list,
    };
    pub use crate::{
        BoxShadow, ButtonVariant, ClickEvent, Context, Cursor, Entity, FontFamily, FontStyle,
        FontWeight, HoverEvent, Image, ImageAlpha, ImageFilter, ImageFit, InputResult, IntoElement,
        KeyEvent, KeyInput, KeyboardKey, Listener, Modifiers, PaintStyle, PointerButton,
        PointerButtons, PointerCancelReason, PointerEvents, PointerInput, Runtime, SemanticRole,
        TextAlign, TextChangeEvent, TextSubmitEvent, Theme, ThemeColor, Ui, View, ViewContext,
        WheelInput, button, column, image, label, rgb8, rgba8, row, stack, text_input,
    };
    #[cfg(all(feature = "desktop-services", not(target_arch = "wasm32")))]
    pub use crate::{DesktopError, DesktopResult};
    #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
    pub use crate::{
        DialogError, DialogResult, DialogTask, FileDialog, MessageButtons, MessageDialog,
        MessageLevel, MessageResponse,
    };
    pub use crate::{
        Dock, DockContextEvent, DockDragInfo, DockDropPreview, DockDropTarget, DockError,
        DockEvent, DockNode, DockNodeId, DockPanel, DockSide, DockSplit, DockTabs, DockTree, dock,
        dock_panel,
    };
    pub use crate::{
        FocusError, FocusHandle, FocusPlacement, FocusScope, Key, Tab, TabActivation,
        TabCloseEvent, TabContentPolicy, TabSelectEvent, Tabs, tab, tabs,
    };
    #[cfg(feature = "native-menus")]
    pub use crate::{NativeMenu, NativeMenuBar, NativeMenuRole};
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

#[cfg(all(test, feature = "layout"))]
mod image_tests;

#[cfg(all(test, feature = "layout"))]
mod layout_tests;

#[cfg(all(test, feature = "layout"))]
mod input_tests;
#[cfg(all(test, feature = "layout"))]
mod overlay_command_tests;

#[cfg(all(test, feature = "layout"))]
mod control_tests;

#[cfg(all(test, feature = "layout"))]
mod dock_tests;
#[cfg(all(test, feature = "layout"))]
mod focus_tab_tests;

#[cfg(all(test, feature = "layout"))]
mod virtual_list_tests;
