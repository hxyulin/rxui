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
//! tasks feature delivers owned background and local results in fresh UI updates; native
//! adds Application hosting over astrelis-winit with lazy AccessKit publication.
//! Optional native-dialogs adds parent-bound file pickers and message dialogs;
//! desktop-services adds asynchronous URL/file launches and file-manager reveal.
//! Native clipboard text access shares the host's persistent clipboard owner.
//! Native window geometry snapshots support application-owned persistence and
//! monitor-aware restoration; file-drop hooks deliver window-level native paths.
//! The native host follows the system appearance, accent and accessibility
//! preferences, draws focus rings after keyboard navigation and supports custom
//! title bars through window drag regions.
//! Portable semantics are part of layout; the optional accessibility feature adds
//! AccessKit translation for custom hosts. Fixed- and variable-height virtual lists describe only
//! viewport rows plus overscan; shared windows retain independent scroll offsets.
//! Variable-height lists keep the first visible row in place when rows above it change.
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
mod animation;
#[cfg(feature = "layout")]
mod appearance;
#[cfg(feature = "layout")]
mod commands;
mod context;
#[cfg(feature = "layout")]
mod controls;
#[cfg(feature = "layout")]
mod conventions;
#[cfg(feature = "layout")]
mod custom;
#[cfg(feature = "layout")]
mod dock;
#[cfg(feature = "layout")]
mod dock_policy;
#[cfg(feature = "layout")]
mod dock_transfer;
#[cfg(feature = "layout")]
mod dock_view;
#[cfg(feature = "layout")]
mod dock_workspace;
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
mod standard;
#[cfg(feature = "layout")]
mod tabs;
#[cfg(feature = "tasks")]
mod tasks;
#[cfg(feature = "rendering")]
mod testing;
#[cfg(feature = "layout")]
mod theme;
#[cfg(feature = "layout")]
mod tree;
#[cfg(feature = "layout")]
mod typography;
#[cfg(feature = "layout")]
mod ui;
#[cfg(feature = "layout")]
mod virtual_list;
#[cfg(feature = "layout")]
mod virtual_rows;

#[cfg(feature = "accessibility")]
pub use accessibility::{AccessKitStats, AccessKitTree};
#[cfg(feature = "accessibility")]
pub use accesskit;
pub use animation::{Animated, Easing, Interpolate, Spring, ThemeMotion, Transition};
#[cfg(feature = "layout")]
pub use appearance::{ColorScheme, SystemPreferences, ThemeMode, ThemeSet};
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
    Axis, RangeInfo, ResizeEvent, ResizePhase, ScrollArea, ScrollbarStyle, Split, SplitPosition,
    scroll_area, scrollbar, split_column, split_row,
};
#[cfg(feature = "layout")]
pub use conventions::{
    DialogButtonOrder, DialogButtonRole, FocusRingPolicy, system_ui_font_families,
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
pub use dock_policy::DockDropQuery;
#[cfg(feature = "layout")]
pub use dock_transfer::{DockDetachEvent, DockTransferEvent};
#[cfg(feature = "layout")]
pub use dock_view::{Dock, DockContextEvent, DockPanel, dock, dock_panel, dock_panel_lazy};
#[cfg(feature = "layout")]
pub use dock_workspace::{DockSurfaceId, DockTransfer, DockWorkspace, DockWorkspaceError};
#[cfg(feature = "layout")]
pub use editing::SubmitShortcut;
#[cfg(feature = "layout")]
pub use editing::{
    TextAffinity, TextChangeEvent, TextInputEvent, TextInputInfo, TextMovement, TextPosition,
    TextSelection, TextSubmitEvent,
};
#[cfg(feature = "layout")]
pub use element::rich_text;
#[cfg(feature = "layout")]
pub use element::text_area;
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
    Cursor, EventPhase, FocusEvent, HoverEvent, ImeCancelReason, ImeEvent, ImeInput, InputResult,
    KeyEvent, KeyInput, KeyboardKey, Modifiers, PhysicalKey, PointerButton, PointerButtons,
    PointerCancelReason, PointerInput, TextInsertInput, TextInsertSource, WheelInput, WheelSource,
};
pub use listener::{Dispatch, Listener};
#[cfg(feature = "native")]
pub use native::{
    Application, ApplicationError, ApplicationWakeHandle, GraphicsPrepareContext, PointerLock,
    WindowHandle, WindowId, WindowOptions,
};
#[cfg(feature = "native")]
pub use native::{FileDropEvent, WindowGeometry, WindowInput, WindowInputResult};
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
pub use scrolling::{ScrollBehavior, ScrollError, ScrollHandle, ScrollPlacement, ScrollState};
#[cfg(feature = "layout")]
pub use semantics::{
    AccessibilityHandle, Checked, LivePoliteness, NumericValue, SemanticAction, SemanticLink,
    SemanticNode, SemanticRole,
};
#[cfg(feature = "layout")]
pub use standard::{
    Choice, ChoiceEvent, ProgressBar, RadioGroup, Select, Slider, SliderEvent, SliderPhase,
    TOOLTIP_DELAY, Toggle, ToggleEvent, checkbox, choice, progress_bar, radio_group, select,
    select_keyed, slider, spinner, switch, tooltip,
};
#[cfg(feature = "layout")]
pub use tabs::{
    Tab, TabActivation, TabCloseEvent, TabContentPolicy, TabSelectEvent, Tabs, tab, tabs,
};
#[cfg(feature = "tasks")]
pub use tasks::{
    BackgroundFuture, BlockingJob, LocalFuture, LocalTask, LocalTaskExecutor, SpawnError, Task,
    TaskError, TaskExecutor, TaskResult, TaskScope,
};
#[cfg(all(feature = "tasks", not(target_arch = "wasm32")))]
pub use tasks::{ThreadPoolExecutor, sleep};
#[cfg(feature = "rendering")]
pub use testing::TestUi;
#[cfg(feature = "layout")]
pub use theme::{
    BoxShadow, PaintStyle, ResolvedPaint, ResolvedShadow, StyleColor, Theme, ThemeColor,
    ThemeColors, ThemeMetrics, rgb8, rgba8,
};
#[cfg(feature = "layout")]
pub use tree::{
    TreeCommand, TreeData, TreeDataError, TreeNode, TreeProposal, TreeRow, TreeSelectionMode,
    TreeState, virtual_tree,
};
#[cfg(feature = "layout")]
pub use typography::{FontFace, LinkEvent, ResolvedSpan, TextSpan, link, span};
#[cfg(feature = "layout")]
pub use typography::{FontFamily, FontStyle, FontWeight, TextAlign, TextOverflow, TextStyle};
#[cfg(feature = "layout")]
pub use ui::dock_dispatch::{DockDragInfo, DockDragSession, DockDropPreview};
#[cfg(feature = "layout")]
pub use ui::editing_transfer::{
    TextEditingCapture, TextEditingError, TextEditingHandle, TextEditingIdentity,
    TextEditingSnapshot,
};
#[cfg(feature = "layout")]
pub use ui::focus::{FocusError, FocusHandle, FocusPlacement, FocusScope};
#[cfg(feature = "layout")]
pub use ui::{
    Bounds, ElementId, ElementInfo, ElementType, LinkInfo, PointerEvent, RoundedClip, TextMeasure,
    TextRequest, TextWidth, Ui, UiError, UiStats,
};
#[cfg(feature = "layout")]
pub use virtual_list::{VariableList, VirtualList, variable_list, virtual_list};
#[cfg(feature = "layout")]
pub use virtual_rows::VirtualRows;

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
        AccessibilityHandle, BoxShadow, ButtonVariant, Checked, ClickEvent, Context, Cursor,
        Entity, FocusEvent, FontFamily, FontStyle, FontWeight, HoverEvent, Image, ImageAlpha,
        ImageFilter, ImageFit, InputResult, IntoElement, KeyEvent, KeyInput, KeyboardKey, Listener,
        LivePoliteness, Modifiers, PaintStyle, PhysicalKey, PointerButton, PointerButtons,
        PointerCancelReason, PointerEvents, PointerInput, Runtime, SemanticRole, TextAlign,
        TextChangeEvent, TextOverflow, TextSubmitEvent, Theme, ThemeColor, Ui, View, ViewContext,
        WheelInput, WheelSource, button, column, image, label, rgb8, rgba8, row, stack, text_input,
    };
    pub use crate::{
        AnchorHandle, Command, CommandAction, CommandId, CommandInfo, CommandRegistration,
        CommandStatus, DismissEvent, DismissReason, Overlay, OverlayAnchor, PopoverPlacement,
        Shortcut, menu, menu_item, modal, popover, standard_commands,
    };
    pub use crate::{Animated, Easing, Interpolate, Spring, Transition};
    #[cfg(feature = "native")]
    pub use crate::{
        Application, ApplicationError, ApplicationWakeHandle, CloseResponse, FileDropEvent,
        PointerLock, WindowGeometry, WindowHandle, WindowOptions,
    };
    pub use crate::{
        Axis, RangeInfo, ResizeEvent, ResizePhase, ScrollArea, ScrollAxes, ScrollBehavior,
        ScrollError, ScrollHandle, ScrollPlacement, ScrollState, Split, SplitPosition, TreeCommand,
        TreeData, TreeDataError, TreeNode, TreeProposal, TreeRow, TreeSelectionMode, TreeState,
        VariableList, VirtualList, VirtualRows, scroll_area, scrollbar, split_column, split_row,
        variable_list, virtual_list, virtual_tree,
    };
    pub use crate::{
        Choice, ChoiceEvent, ProgressBar, RadioGroup, Select, Slider, SliderEvent, SliderPhase,
        Toggle, ToggleEvent, checkbox, choice, progress_bar, radio_group, select, select_keyed,
        slider, spinner, switch, tooltip,
    };
    pub use crate::{
        ColorScheme, DialogButtonOrder, DialogButtonRole, FocusRingPolicy, ScrollbarStyle,
        SystemPreferences, ThemeMode, ThemeSet,
    };
    #[cfg(all(feature = "desktop-services", not(target_arch = "wasm32")))]
    pub use crate::{DesktopError, DesktopResult};
    #[cfg(all(feature = "native-dialogs", not(target_arch = "wasm32")))]
    pub use crate::{
        DialogError, DialogResult, DialogTask, FileDialog, MessageButtons, MessageDialog,
        MessageLevel, MessageResponse,
    };
    pub use crate::{
        Dock, DockContextEvent, DockDetachEvent, DockDragInfo, DockDragSession, DockDropPreview,
        DockDropQuery, DockDropTarget, DockError, DockEvent, DockNode, DockNodeId, DockPanel,
        DockSide, DockSplit, DockSurfaceId, DockTabs, DockTransfer, DockTransferEvent, DockTree,
        DockWorkspace, DockWorkspaceError, dock, dock_panel, dock_panel_lazy,
    };
    pub use crate::{
        FocusError, FocusHandle, FocusPlacement, FocusScope, Key, Tab, TabActivation,
        TabCloseEvent, TabContentPolicy, TabSelectEvent, Tabs, tab, tabs,
    };
    pub use crate::{
        FontFace, LinkEvent, SubmitShortcut, TextSpan, link, rich_text, span, text_area,
    };
    pub use crate::{ImeCancelReason, ImeEvent, ImeInput, TextInsertInput, TextInsertSource};
    #[cfg(feature = "tasks")]
    pub use crate::{LocalTask, Task, TaskError, TaskResult};
    #[cfg(feature = "native-menus")]
    pub use crate::{NativeMenu, NativeMenuBar, NativeMenuRole};
    pub use crate::{
        TextEditingCapture, TextEditingError, TextEditingHandle, TextEditingIdentity,
        TextEditingSnapshot,
    };
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
mod text_client_tests;

#[cfg(all(test, feature = "layout"))]
mod control_tests;

#[cfg(all(test, feature = "layout"))]
mod dock_surface_drag_tests;
#[cfg(all(test, feature = "layout"))]
mod dock_tests;
#[cfg(all(test, feature = "layout"))]
mod dock_workspace_tests;
#[cfg(all(test, feature = "layout"))]
mod focus_tab_tests;

#[cfg(all(test, feature = "layout"))]
mod virtual_list_tests;

#[cfg(all(test, feature = "layout"))]
mod platform_tests;

#[cfg(all(test, feature = "layout"))]
mod animation_tests;

#[cfg(all(test, feature = "layout"))]
mod scroll_motion_tests;
#[cfg(all(test, feature = "layout"))]
mod tree_tests;
