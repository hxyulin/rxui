//! The API-loss gate: an explicit import of every name `rxui` intends to export.
//!
//! This file asserts nothing at runtime and is not supposed to. It exists to
//! *compile*, and it fails the moment a public name disappears or moves - which
//! is the risk that matters while the crate graph is being recut, because adding
//! a name is visible in review and removing one is not.
//!
//! It is deliberately not `cargo public-api`: that needs a nightly toolchain and
//! catches additions as well, which produces churn on every intentional feature.
//! A plain `use` list needs no extra tooling and answers the only question worth
//! blocking a merge on.
//!
//! Feature-gated modules are imported under the same `cfg` the facade uses, so
//! this file is also what proves each optional surface is reachable at all.
//!
//! Two names are absent on purpose and must stay absent:
//! `rxui::core` (the former blanket re-export of the whole engine) and
//! `DockAxis`/`dock_axis` (a two-variant shadow of `Axis`). `no_removed_surface`
//! at the bottom records why.

#![allow(unused_imports)]

// ---------------------------------------------------------------------------
// Layer 1 - the root.
// ---------------------------------------------------------------------------

use rxui::{
    Alignment, Axis, ButtonStyle, ButtonVariant, ColorRole, Component, ComponentContext,
    ComponentHost, ComponentRuntime, ComponentWithProps, ContainerStyle, FrameStyle, Icon,
    IconButtonStyle, LabelStyle, Space, StackStyle, Theme, View, ViewKey,
};
use rxui::{
    button, button_with, checkbox, column, column_with, component, flex, icon, icon_button, label,
    label_with_style, label_with_width, panel, panel_with_semantics, row, row_with, scroll,
    scroll_at, slider, slider_with_step, spacer, split_pane, stack, stack_with, text_field, views,
};

// ---------------------------------------------------------------------------
// Layer 2 - the grouped surface.
// ---------------------------------------------------------------------------

use rxui::view::{
    ActionEmitter, AnyView, DynamicViews, IntoChildren, Mounted, MountedChildren, MountedState,
    RebuildContext, RetainedSpec, RouteContext, ViewContext, ViewHost, ViewKind, ViewNode,
    retained,
};

use rxui::controls::{
    Choice, ComboOption, combo_box, icon_button_with, numeric_field, radio_group,
};

use rxui::forms::{
    FormValidation, ValidationIssue, ValidationResult, ValidationSeverity, form_section,
    validated_text_field,
};

use rxui::surfaces::{
    CommandItem, CommandPaletteNavigation, DialogAction, Toast, ToastLevel, ToolbarItem,
    command_palette, dialog, toasts, toolbar,
};

use rxui::data::{
    PropertyField, TableRow, TreeRow, editable_property_grid, property_grid,
    render_view_placeholder, virtual_table, virtual_table_with_widths, virtual_tree,
};

use rxui::media::{
    CompositorViewId, ExternalImage, Image, ImageAlignment, ImageFit, ImageSampling, ImageSpec,
    RenderViewContent, RenderViewSpec, image, render_view,
};

use rxui::icons::{IconError, IconSpec, check, chevron_down, close, save, search, settings};

use rxui::style::{
    ButtonStyle as _, ButtonVariant as _, ColorRole as _, ContainerStyle as _, FrameStyle as _,
    IconButtonStyle as _, LabelStyle as _, Space as _, StackStyle as _, Theme as _,
};

use rxui::services::{
    BackgroundCompletion, BackgroundTaskRequest, BackgroundWork, Clipboard, ClipboardReadRequest,
    ComponentServiceRequest, MemoryClipboard, ServiceAction, UndoHistory,
};

use rxui::inspect::{InspectionNode, InspectionSnapshot, ViewStats};

// ---------------------------------------------------------------------------
// Layer 3 - the closed engine vocabulary.
// ---------------------------------------------------------------------------

use rxui::color::{Color, Rgba8};
use rxui::geometry::{
    Affine2, Logical, LogicalPoint, LogicalRect, LogicalSize, Physical, PhysicalPoint,
    PhysicalRect, PhysicalSize, Point, Rect, Size,
};
use rxui::input::{
    CursorIcon, DeviceId, ElementState, ImeEvent, Key, KeyCode, KeyLocation, KeyboardInput,
    Modifiers, NamedKey, PhysicalKey, PointerButton, ScrollDelta, UiInput, key, named_key,
    pointer_moved, pointer_pressed, pointer_released, pointer_wheel, text, with_modifiers,
};
use rxui::paint::{
    Brush, FillRule, PaintError, Painter, Path, PathBuilder, PathVerb, RoundedRect, StrokeStyle,
};
use rxui::semantics::{
    SemanticAction, SemanticActionKind, SemanticData, SemanticNode, SemanticRole,
};

use rxui::engine::{
    Constraints, Element, EventResult, Invalidation, KeyedShapingMemo, LayoutContext, NodeHandle,
    NodeId, PassStats, ShapingMemo, TextLayout, TextLayoutRequest, TextStyle, TextWrap, UiError,
    UiRoot,
};

// ---------------------------------------------------------------------------
// Layer 4 - optional surfaces, each behind the feature it is named after.
// ---------------------------------------------------------------------------

#[cfg(feature = "charts")]
use rxui::charts::{
    ChartAction, ChartElement, ChartOptions, ChartPoint, ChartSeries, ChartSeriesKind, ChartSpec,
    chart, decimate_line,
};

#[cfg(feature = "graph")]
use rxui::graph::{
    GraphEdge, GraphNode, GraphViewport, NodeGraphAction, NodeGraphElement, NodeGraphSpec,
    node_graph,
};

#[cfg(feature = "docking")]
use rxui::docking::{DockNode, DockPane, dock_workspace};

#[cfg(feature = "devtools")]
use rxui::devtools::inspection_view;

#[cfg(any(feature = "native", feature = "native-embedded"))]
use rxui::native::{
    AccessibilityAdapter, AccessibilityRequest, App, AppContext, CommandEncoder, ComponentWindow,
    CompositionStats, GraphicsContext, HostError, HostUpdate, RenderStats, TextureView,
    ViewOptions, ViewRenderTarget, Window, WindowAttributes, WindowEvent, WindowHost,
    WindowHostOptions,
};
#[cfg(all(not(target_arch = "wasm32"), feature = "native"))]
use rxui::native::{ComponentApplication, Runtime, RuntimeConfig, RuntimeError, run_component};

// ---------------------------------------------------------------------------
// The prelude, imported separately: a glob in the same scope as the explicit
// list above would silently paper over a name the list expected at the root.
// ---------------------------------------------------------------------------

mod prelude_is_reachable {
    use rxui::prelude::*;

    /// The prelude's whole claim is that it is enough to write a component.
    /// Writing one here is the only way to check that, and it also pins the
    /// prelude's real boundary: this compiles with no other import, and adding
    /// anything to it that needs one would be a mistake.
    struct Field {
        value: String,
    }

    impl Component for Field {
        type Action = String;
        type Effect = ();

        fn update(&mut self, action: String, _context: &mut ComponentContext<'_, ()>) {
            self.value = action;
        }

        fn view(&self, _theme: &Theme) -> View<String> {
            column_with(
                ContainerStyle::new()
                    .gap(Space::Sm)
                    .background(ColorRole::Surface),
                (
                    label("Name"),
                    text_field("Name", self.value.clone(), |value| value),
                ),
            )
        }
    }

    #[test]
    fn the_prelude_alone_is_enough_to_mount_a_component() {
        let host = rxui::ComponentHost::new(
            Field {
                value: "Ada".into(),
            },
            LogicalSize::new(320.0, 120.0),
            Theme::dark(),
        )
        .expect("the component mounts");
        assert!(
            host.ui()
                .semantic_snapshot()
                .iter()
                .any(|node| node.data.value.as_deref() == Some("Ada"))
        );
    }
}

/// Records the two surfaces this release removed, so a well-meaning
/// reintroduction has to delete a test that says why not.
///
/// `rxui::core` was a blanket `pub use astrelis_ui_next`: an unbounded,
/// unpublished, self-described proving ground re-exported from a 1.0-track
/// facade. The closed `engine` module above replaces it, and the `geometry`,
/// `color`, `input`, `paint`, and `semantics` modules cover what it never did -
/// which is why every consumer also depended on four Astrelis crates directly.
///
/// `DockAxis` and `dock_axis` were a two-variant shadow of the two-variant
/// [`Axis`] plus a public converter between them.
///
/// Neither can be tested for absence in Rust - a missing name is a compile
/// error, not a value - so this is prose next to the list that would have to
/// grow to bring them back.
#[test]
fn no_removed_surface() {}
