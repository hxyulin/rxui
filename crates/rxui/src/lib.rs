//! Component-native desktop UI framework.
//!
//! # Finding things
//!
//! The root holds the vocabulary an application writes constantly: the
//! [`Component`] contract, [`View`], the container and control builders, and the
//! style tokens. Everything else is grouped into a module, and the hot items are
//! *also* at the root - one item can be named two ways, and the short way is
//! deliberate rather than an oversight.
//!
//! | Module | What it holds |
//! |---|---|
//! | [`view`] | The open view protocol, for defining a view kind of your own |
//! | [`controls`] | Interactive leaves: buttons, fields, sliders, choices |
//! | [`forms`] | Validation state and validated fields |
//! | [`surfaces`] | Dialogs, toasts, toolbars, command palettes |
//! | [`data`] | Property grids, trees, tables |
//! | [`media`] | Images and application-rendered viewports |
//! | [`icons`] | The built-in glyph set |
//! | [`style`] | Theme, roles, spacing, per-view style builders |
//! | [`services`] | Clipboard, background work, undo history |
//! | [`inspect`] | Retained-tree snapshots and framework counters |
//!
//! The engine vocabulary RXUI's own signatures speak is re-exported closed, so
//! nothing here forces a dependency on an Astrelis crate: [`geometry`],
//! [`color`], [`input`], [`semantics`], [`paint`], and [`engine`].
//!
//! # Optional surfaces
//!
//! Each optional module is named after the Cargo feature that enables it, so a
//! missing item names its own fix: [`charts`], [`graph`], [`docking`],
//! [`devtools`], and [`native`].
//!
//! Features are for *applications*. A library should depend on `rxui-core`
//! directly rather than on `rxui` with narrowed features, because Cargo unifies
//! features across a dependency graph - a library that narrows them makes its
//! own surface depend on whatever its consumer chose.
//!
//! # Stability
//!
//! Everything is versioned normally except [`engine`], which is documented
//! semver-exempt while RXUI is pre-1.0 because it belongs to the Astrelis engine
//! developed alongside it.

#![warn(missing_docs)]

// ---------------------------------------------------------------------------
// Root: what an application names constantly.
// ---------------------------------------------------------------------------

pub use rxui_core::{
    Alignment, Axis, Component, ComponentContext, ComponentHost, ComponentRuntime,
    ComponentWithProps, View, ViewKey,
};

pub use rxui_core::{
    button, button_with, checkbox, column, column_with, component, flex, icon, icon_button, label,
    label_with_style, label_with_width, panel, panel_with_semantics, row, row_with, scroll,
    scroll_at, slider, slider_with_step, spacer, split_pane, stack, stack_with, text_field, views,
};

pub use rxui_core::{
    ButtonStyle, ButtonVariant, ColorRole, ContainerStyle, FrameStyle, Icon, IconButtonStyle,
    IconSpec, LabelStyle, Space, StackStyle, Theme,
};

// ---------------------------------------------------------------------------
// Layer 2: the complete grouped surface.
// ---------------------------------------------------------------------------

/// The open view protocol, and every container and text builder.
///
/// Read this module to define a view kind of your own - a container with its own
/// layout policy, or a leaf over a retained element. RXUI's own twenty kinds are
/// written against exactly this surface with no privileged access, which is what
/// makes it demonstrably sufficient. See `docs/view-protocol.md`.
pub mod view {
    pub use rxui_core::{
        ActionEmitter, AnyView, DynamicViews, IntoChildren, Mounted, MountedChildren, MountedState,
        RebuildContext, RetainedSpec, RouteContext, View, ViewContext, ViewHost, ViewKey, ViewKind,
        ViewNode, column, column_with, component, flex, label, label_with_style, label_with_width,
        panel, panel_with_semantics, retained, row, row_with, scroll, scroll_at, spacer,
        split_pane, stack, stack_with, views,
    };
}

/// Interactive leaves.
///
/// Every control here is *controlled*: it renders the value its component
/// declares and reports an intent to change it. Refusing that intent in
/// `update` reverts the control, which is what makes a validating field work.
pub mod controls {
    pub use rxui_core::controls::{Choice, ComboOption, combo_box, numeric_field, radio_group};
    pub use rxui_core::{
        button, button_with, checkbox, icon_button, icon_button_with, slider, slider_with_step,
        text_field,
    };
}

/// Validation state and the fields that present it.
pub mod forms {
    pub use rxui_core::forms::{
        FormValidation, ValidationIssue, ValidationResult, ValidationSeverity, form_section,
        validated_text_field,
    };
}

/// Application surfaces that carry policy of their own.
///
/// Focus scoping, escape dismissal, and query matching live here rather than in
/// [`crate::controls`] because they are decisions, not presentation.
pub mod surfaces {
    pub use rxui_core::surfaces::{
        CommandItem, CommandPaletteNavigation, DialogAction, Toast, ToastLevel, ToolbarItem,
        command_palette, dialog, toasts, toolbar,
    };
}

/// Keyed data presentation.
///
/// Each builder takes a caller-owned visible range and narrows it onto the rows
/// that exist, so a scroll offset computed at runtime can name rows past the end
/// without panicking.
pub mod data {
    pub use rxui_core::data::{
        PropertyField, TableRow, TreeRow, editable_property_grid, property_grid,
        render_view_placeholder, virtual_table, virtual_table_with_widths, virtual_tree,
    };
}

/// Raster images and application-rendered viewports.
pub mod media {
    pub use rxui_core::media::{
        CompositorViewId, ExternalImage, Image, ImageAlignment, ImageFit, ImageSampling, ImageSpec,
        RenderViewContent, RenderViewSpec, image, render_view,
    };
}

/// The built-in glyph set.
pub mod icons {
    pub use rxui_core::icons::{check, chevron_down, close, save, search, settings};
    pub use rxui_core::{Icon, IconButtonStyle, IconError, IconSpec, icon};
}

/// Theme tokens and per-view style builders.
pub mod style {
    pub use rxui_core::{
        ButtonStyle, ButtonVariant, ColorRole, ContainerStyle, FrameStyle, IconButtonStyle,
        LabelStyle, Space, StackStyle, Theme,
    };
}

/// Side effects a component requests and the host performs.
pub mod services {
    pub use rxui_core::{
        BackgroundCompletion, BackgroundTaskRequest, BackgroundWork, Clipboard,
        ClipboardReadRequest, ComponentServiceRequest, MemoryClipboard, ServiceAction, UndoHistory,
    };
}

/// Retained-tree snapshots and framework work counters.
pub mod inspect {
    pub use rxui_core::diagnostics::ViewStats;
    pub use rxui_core::inspect::{InspectionNode, InspectionSnapshot};
}

// ---------------------------------------------------------------------------
// Layer 3: the closed engine vocabulary.
// ---------------------------------------------------------------------------

pub use rxui_core::{color, engine, geometry, input, paint, semantics};

// ---------------------------------------------------------------------------
// Layer 4: optional surfaces, each module named after its feature.
// ---------------------------------------------------------------------------

/// Interactive charts. Requires the `charts` feature.
#[cfg(feature = "charts")]
#[cfg_attr(docsrs, doc(cfg(feature = "charts")))]
pub mod charts {
    pub use rxui_widgets::element::chart::{
        ChartAction, ChartElement, ChartOptions, ChartPoint, ChartSeries, ChartSeriesKind,
        ChartSpec, chart, decimate_line,
    };
}

/// Interactive node graphs. Requires the `graph` feature.
#[cfg(feature = "graph")]
#[cfg_attr(docsrs, doc(cfg(feature = "graph")))]
pub mod graph {
    pub use rxui_widgets::element::graph::{
        GraphEdge, GraphNode, GraphViewport, NodeGraphAction, NodeGraphElement, NodeGraphSpec,
        node_graph,
    };
}

/// Splittable, tabbed dock workspaces. Requires the `docking` feature.
#[cfg(feature = "docking")]
#[cfg_attr(docsrs, doc(cfg(feature = "docking")))]
pub mod docking {
    pub use rxui_widgets::compose::docking::{DockNode, DockPane, dock_workspace};
}

/// An interactive viewer for [`crate::inspect`] snapshots. Requires the
/// `devtools` feature.
#[cfg(feature = "devtools")]
#[cfg_attr(docsrs, doc(cfg(feature = "devtools")))]
pub mod devtools {
    pub use rxui_widgets::inspector::inspection_view;
}

/// Native windows, and the windowing vocabulary they are opened with.
///
/// Requires `native` (which drives a winit event loop) or `native-embedded`
/// (which does not, for an application that owns its own loop).
#[cfg(any(feature = "native", feature = "native-embedded"))]
#[cfg_attr(docsrs, doc(cfg(feature = "native")))]
pub mod native {
    pub use rxui_native::*;
}

// ---------------------------------------------------------------------------
// Prelude.
// ---------------------------------------------------------------------------

/// Enough to write a component, and nothing more.
///
/// Deliberately small. It used to be a byte-identical copy of the root glob,
/// which made it useless: importing it told a reader nothing about what the file
/// actually used.
pub mod prelude {
    pub use crate::{
        ColorRole, Component, ComponentContext, ComponentWithProps, ContainerStyle, Space, Theme,
        View, button, checkbox, column, column_with, component, geometry::LogicalSize, label, row,
        row_with, text_field, views,
    };
}
