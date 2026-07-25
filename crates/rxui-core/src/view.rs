//! Lightweight view values and keyed reconciliation.

use std::{
    any::{Any, TypeId},
    cell::RefCell,
    cmp::Ordering as CmpOrdering,
    collections::HashMap,
    fmt,
    hash::{Hash, Hasher},
    ops::RangeInclusive,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize},
};
use astrelis_ui_next::{
    Align, Alignment, Axis, BoxElement, Button, ButtonIcon, Checkbox, Element, Flex, Frame,
    Invalidation, KeyListener, Label, NodeHandle, NodeId, Scroll, ScrollAxis, SemanticData, Slider,
    SplitPane, Stack, TextField, UiError, UiRoot,
};

use crate::{
    ButtonVariant, ColorRole, ComponentContext, ComponentWithProps, DirtyHandle, Icon,
    IconButtonStyle, RenderScope, RoutedComponentAction, Space, Theme, diagnostics::ViewStats,
};

static NEXT_COMPONENT_ID: AtomicU64 = AtomicU64::new(1);

/// Stable view identity used by dynamic collections.
///
/// Keys are compared and hashed on every keyed reconciliation pass, so the
/// representation avoids allocating for the two idiomatic sources of identity: a
/// numeric domain id (`.key(item.id)`) and a `&'static str` literal
/// (`.key("header")`). Only an owned or borrowed non-static string allocates,
/// and then only once per key rather than once per frame.
///
/// String-shaped keys compare and hash by their text regardless of whether they
/// are borrowed or owned, so a collection keyed with a literal one frame and a
/// `String` the next keeps its retained identity. [`ViewKey::Index`] is a
/// distinct space: `ViewKey::from(7u64)` is *not* equal to `ViewKey::from("7")`.
#[derive(Clone, Debug)]
pub enum ViewKey {
    /// Numeric domain identity.
    Index(u64),
    /// Borrowed static identity, typically a literal.
    Name(&'static str),
    /// Shared owned identity.
    Owned(Arc<str>),
}

impl ViewKey {
    /// Creates an owned key.
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self::Owned(value.into())
    }

    /// Returns the textual form of a string-shaped key.
    fn text(&self) -> Option<&str> {
        match self {
            Self::Index(_) => None,
            Self::Name(value) => Some(value),
            Self::Owned(value) => Some(value),
        }
    }
}

impl PartialEq for ViewKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Index(left), Self::Index(right)) => left == right,
            (Self::Index(_), _) | (_, Self::Index(_)) => false,
            (left, right) => left.text() == right.text(),
        }
    }
}

impl Eq for ViewKey {}

impl Hash for ViewKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // The discriminant byte keeps the numeric and textual spaces apart; both
        // textual variants hash as `str` so they collide only when equal.
        match self {
            Self::Index(value) => {
                0u8.hash(state);
                value.hash(state);
            }
            Self::Name(value) => {
                1u8.hash(state);
                (*value).hash(state);
            }
            Self::Owned(value) => {
                1u8.hash(state);
                (**value).hash(state);
            }
        }
    }
}

impl PartialOrd for ViewKey {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for ViewKey {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        match (self, other) {
            (Self::Index(left), Self::Index(right)) => left.cmp(right),
            (Self::Index(_), _) => CmpOrdering::Less,
            (_, Self::Index(_)) => CmpOrdering::Greater,
            (left, right) => left.text().cmp(&right.text()),
        }
    }
}

impl From<&'static str> for ViewKey {
    fn from(value: &'static str) -> Self {
        Self::Name(value)
    }
}

impl From<String> for ViewKey {
    fn from(value: String) -> Self {
        Self::Owned(Arc::<str>::from(value))
    }
}

impl From<Arc<str>> for ViewKey {
    fn from(value: Arc<str>) -> Self {
        Self::Owned(value)
    }
}

impl From<u64> for ViewKey {
    fn from(value: u64) -> Self {
        Self::Index(value)
    }
}

impl fmt::Display for ViewKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Index(value) => value.fmt(formatter),
            Self::Name(value) => value.fmt(formatter),
            Self::Owned(value) => value.fmt(formatter),
        }
    }
}

/// One lightweight typed-action view value.
pub struct AnyView<Action: 'static> {
    key: Option<ViewKey>,
    inner: Box<dyn DynView<Action>>,
}

/// Application-facing lightweight view description.
///
/// The `AnyView` name remains available for low-level integrations, while
/// component APIs use this alias to avoid exposing the erasure strategy.
pub type View<Action> = AnyView<Action>;

impl<Action: 'static> AnyView<Action> {
    /// Assigns stable identity for reconciliation inside a dynamic sequence.
    pub fn key(self, key: impl Into<ViewKey>) -> Self {
        self.keyed(key)
    }

    /// Assigns stable identity for reconciliation inside a dynamic sequence.
    pub fn keyed(mut self, key: impl Into<ViewKey>) -> Self {
        self.key = Some(key.into());
        self
    }

    fn kind(&self) -> TypeId {
        self.inner.kind()
    }

    /// Nests a view with a local action type and maps it into its parent.
    pub fn map_action<Parent: 'static>(
        self,
        map: impl Fn(Action) -> Parent + 'static,
    ) -> AnyView<Parent> {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(MapActionView {
                child: self,
                map: Arc::new(map),
            }),
        }
    }

    /// Controls interaction for this view and its retained descendants.
    pub fn enabled(self, enabled: bool) -> Self {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(EnabledView {
                child: self,
                enabled,
            }),
        }
    }

    /// Controls retained visibility without discarding subtree state.
    pub fn visible(self, visible: bool) -> Self {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(VisibleView {
                child: self,
                visible,
            }),
        }
    }

    /// Wraps this view in an explicit sizing and flex-growth boundary.
    pub fn frame(self, style: FrameStyle) -> Self {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(FrameView { child: self, style }),
        }
    }

    /// Expands to available space and positions this view within it.
    pub fn aligned(self, alignment: Alignment, padding: Space) -> Self {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(AlignView {
                child: self,
                alignment,
                padding,
            }),
        }
    }

    /// Autofocuses this subtree while active and restores prior focus on exit.
    pub fn focus_scope(self, active: bool) -> Self {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(FocusScopeView {
                child: self,
                active,
            }),
        }
    }

    /// Handles Escape after an unhandled key event bubbles from this subtree.
    pub fn dismiss_on_escape(self, action: Action) -> Self
    where
        Action: Clone,
    {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(EscapeView {
                child: self,
                action,
            }),
        }
    }

    /// Handles list navigation and submission after focused-child bubbling.
    pub fn command_navigation(self, previous: Action, next: Action, submit: Action) -> Self
    where
        Action: Clone,
    {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(CommandNavigationView {
                child: self,
                previous,
                next,
                submit,
            }),
        }
    }
}

/// Explicit sizing and flex-growth options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameStyle {
    /// Optional preferred width.
    pub width: Option<f32>,
    /// Optional preferred height.
    pub height: Option<f32>,
    /// Minimum size.
    pub min: LogicalSize,
    /// Optional maximum size.
    pub max: Option<LogicalSize>,
    /// Relative share of remaining flex space.
    pub grow: f32,
}

impl FrameStyle {
    /// Creates an unconstrained, non-growing frame.
    pub const fn new() -> Self {
        Self {
            width: None,
            height: None,
            min: LogicalSize::ZERO,
            max: None,
            grow: 0.0,
        }
    }

    /// Selects preferred width.
    pub const fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Selects preferred height.
    pub const fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Selects relative main-axis growth.
    pub const fn grow(mut self, grow: f32) -> Self {
        self.grow = grow;
        self
    }

    /// Selects minimum size.
    pub const fn min(mut self, min: LogicalSize) -> Self {
        self.min = min;
        self
    }

    /// Selects maximum size.
    pub const fn max(mut self, max: LogicalSize) -> Self {
        self.max = Some(max);
        self
    }
}

impl Default for FrameStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// Converts static or collected child views into a container's child list.
pub trait IntoChildren<Action: 'static> {
    /// Produces child views in paint and focus order.
    fn into_children(self) -> Vec<AnyView<Action>>;
}

impl<Action: 'static> IntoChildren<Action> for Vec<AnyView<Action>> {
    fn into_children(self) -> Vec<AnyView<Action>> {
        self
    }
}

impl<Action: 'static, const N: usize> IntoChildren<Action> for [AnyView<Action>; N] {
    fn into_children(self) -> Vec<AnyView<Action>> {
        Vec::from(self)
    }
}

/// Collected dynamic children created by [`views`].
pub struct DynamicViews<Action: 'static>(Vec<AnyView<Action>>);

impl<Action: 'static> IntoChildren<Action> for DynamicViews<Action> {
    fn into_children(self) -> Vec<AnyView<Action>> {
        self.0
    }
}

/// Collects an iterator of dynamic views for use in a container.
pub fn views<Action: 'static>(
    children: impl IntoIterator<Item = AnyView<Action>>,
) -> DynamicViews<Action> {
    DynamicViews(children.into_iter().collect())
}

macro_rules! impl_tuple_children {
    ($($view:ident),+ $(,)?) => {
        impl<Action: 'static> IntoChildren<Action> for (
            $(impl_tuple_children!(@type $view Action),)+
        ) {
            #[allow(non_snake_case)]
            fn into_children(self) -> Vec<AnyView<Action>> {
                let ($($view,)+) = self;
                vec![$($view),+]
            }
        }
    };
    (@type $view:ident $action:ident) => {
        AnyView<$action>
    };
}

impl_tuple_children!(A);
impl_tuple_children!(A, B);
impl_tuple_children!(A, B, C);
impl_tuple_children!(A, B, C, D);
impl_tuple_children!(A, B, C, D, E);
impl_tuple_children!(A, B, C, D, E, F);
impl_tuple_children!(A, B, C, D, E, F, G);
impl_tuple_children!(A, B, C, D, E, F, G, H);

trait DynView<Action: 'static>: 'static {
    fn kind(&self) -> TypeId;

    /// Instrumented entry point for the mount path.
    ///
    /// Every from-scratch mount in this module dispatches through here, so
    /// `nodes_built` counts each view node exactly once regardless of how many
    /// wrapper layers a node sits behind. Call this instead of [`Self::build`].
    fn build_counted(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        // `nodes_built`: one view node mounted from scratch.
        ViewStats::record_node_built();
        self.build(key, context)
    }

    /// Instrumented entry point for the in-place reconciliation path.
    ///
    /// Every in-place reconciliation in this module dispatches through here,
    /// so `nodes_rebuilt` counts each view node exactly once. Call this
    /// instead of [`Self::rebuild`].
    fn rebuild_counted(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        // `nodes_rebuilt`: one view node reconciled against its previous
        // mounted instance rather than replaced.
        ViewStats::record_node_rebuilt();
        self.rebuild(mounted, context)
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError>;
    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError>;
}

struct MountedView<Action: 'static> {
    key: Option<ViewKey>,
    kind: TypeId,
    node: NodeId,
    state: Box<dyn MountedState<Action>>,
    marker: std::marker::PhantomData<fn() -> Action>,
}

pub(crate) struct RouteContext<'a> {
    pub(crate) services: &'a mut Vec<crate::ComponentServiceRequest>,
}

/// Retained-tree access for the depth-ordered dirty drain.
///
/// A dirty nested component carries everything else it needs — its retained
/// parent handle, its action sink, its depth — in its own mounted state, so a
/// scoped rebuild only needs the tree, the theme, and this flush's force flag.
pub(crate) struct RebuildContext<'a> {
    pub(crate) ui: &'a mut UiRoot,
    pub(crate) theme: &'a Theme,
    pub(crate) force: bool,
}

pub(crate) trait MountedState<Action: 'static>: Any {
    fn as_any_mut(&mut self) -> &mut dyn Any;

    fn route(
        &mut self,
        _action: &mut Option<crate::RoutedComponentAction>,
        _context: &mut RouteContext<'_>,
    ) -> Result<Vec<Action>, UiError> {
        Ok(Vec::new())
    }

    /// Rebuilds the nested component identified by `target`, if it lives here.
    ///
    /// Returns whether the target was found, which lets containers stop
    /// descending as soon as the owning subtree has been handled. Leaves cannot
    /// own a component boundary and never find anything.
    fn rebuild_dirty(
        &mut self,
        _target: u64,
        _context: &mut RebuildContext<'_>,
    ) -> Result<bool, UiError> {
        Ok(false)
    }
}

macro_rules! leaf_mounted_state {
    ($state:ty) => {
        impl<Action: 'static> MountedState<Action> for $state {
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        }
    };
}

/// Implements [`MountedState`] for a wrapper that owns exactly one child view.
macro_rules! wrapper_mounted_state {
    ($state:ident $(where Action: $bound:path)?) => {
        impl<Action: 'static $(+ $bound)?> MountedState<Action> for $state<Action> {
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }

            fn route(
                &mut self,
                action: &mut Option<crate::RoutedComponentAction>,
                context: &mut RouteContext<'_>,
            ) -> Result<Vec<Action>, UiError> {
                self.child.state.route(action, context)
            }

            fn rebuild_dirty(
                &mut self,
                target: u64,
                context: &mut RebuildContext<'_>,
            ) -> Result<bool, UiError> {
                self.child.state.rebuild_dirty(target, context)
            }
        }
    };
}

/// Implements [`MountedState`] for a container that owns a reconciled child list.
macro_rules! container_mounted_state {
    ($state:ident) => {
        impl<Action: 'static> MountedState<Action> for $state<Action> {
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }

            fn route(
                &mut self,
                action: &mut Option<crate::RoutedComponentAction>,
                context: &mut RouteContext<'_>,
            ) -> Result<Vec<Action>, UiError> {
                let mut output = Vec::new();
                for child in self.children.mounted_mut() {
                    output.extend(child.state.route(action, context)?);
                    if action.is_none() {
                        break;
                    }
                }
                Ok(output)
            }

            fn rebuild_dirty(
                &mut self,
                target: u64,
                context: &mut RebuildContext<'_>,
            ) -> Result<bool, UiError> {
                for child in self.children.mounted_mut() {
                    if child.state.rebuild_dirty(target, context)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    };
}

struct ViewContext<'a, Action: 'static> {
    ui: &'a mut UiRoot,
    parent: NodeId,
    theme: &'a Theme,
    action_sink: Arc<dyn Fn(Action) -> Box<dyn Any>>,
    /// Component nesting depth of the component whose view is being built.
    depth: u32,
    /// Dirty set shared with the owning [`crate::ComponentRuntime`].
    dirty: &'a DirtyHandle,
    /// Whether this pass must ignore props equality at component boundaries.
    force: bool,
    marker: std::marker::PhantomData<fn() -> Action>,
}

impl<Action: 'static> ViewContext<'_, Action> {
    fn reborrow(&mut self, parent: NodeId) -> ViewContext<'_, Action> {
        ViewContext {
            ui: self.ui,
            parent,
            theme: self.theme,
            action_sink: self.action_sink.clone(),
            depth: self.depth,
            dirty: self.dirty,
            force: self.force,
            marker: std::marker::PhantomData,
        }
    }
}

/// Mounts and incrementally reconciles one root view.
pub struct ViewHost<Action: 'static> {
    mounted: MountedView<Action>,
    /// Hoisted root action sink.
    ///
    /// The root component's actions need no routing wrapper, so this is a single
    /// boxing closure allocated at mount instead of one per rebuild.
    sink: Arc<dyn Fn(Action) -> Box<dyn Any>>,
}

impl<Action: 'static> ViewHost<Action> {
    /// Builds the initial retained subtree.
    pub(crate) fn mount(
        ui: &mut UiRoot,
        theme: &Theme,
        view: AnyView<Action>,
        dirty: &DirtyHandle,
    ) -> Result<Self, UiError> {
        let sink: Arc<dyn Fn(Action) -> Box<dyn Any>> = Arc::new(|action| Box::new(action));
        let mut context = ViewContext {
            parent: ui.root(),
            ui,
            theme,
            action_sink: sink.clone(),
            depth: 0,
            dirty,
            force: false,
            marker: std::marker::PhantomData,
        };
        let mounted = view.inner.build_counted(view.key, &mut context)?;
        Ok(Self { mounted, sink })
    }

    /// Reconciles a replacement root view into retained state.
    pub(crate) fn rebuild(
        &mut self,
        ui: &mut UiRoot,
        theme: &Theme,
        view: AnyView<Action>,
        dirty: &DirtyHandle,
        force: bool,
    ) -> Result<(), UiError> {
        let mut context = ViewContext {
            parent: ui.root(),
            ui,
            theme,
            action_sink: self.sink.clone(),
            depth: 0,
            dirty,
            force,
            marker: std::marker::PhantomData,
        };
        if self.mounted.kind == view.kind() {
            view.inner
                .rebuild_counted(&mut self.mounted, &mut context)?;
        } else {
            context.ui.remove(self.mounted.node)?;
            self.mounted = view.inner.build_counted(view.key, &mut context)?;
        }
        Ok(())
    }

    /// Root retained identity.
    pub const fn node(&self) -> NodeId {
        self.mounted.node
    }

    /// Rebuilds one dirty nested component without touching the root's view.
    ///
    /// A component that has since been unmounted is simply not found, which is
    /// why this reports nothing: a stale dirty entry is not an error.
    pub(crate) fn rebuild_scoped(
        &mut self,
        ui: &mut UiRoot,
        theme: &Theme,
        target: u64,
        force: bool,
    ) -> Result<(), UiError> {
        let mut context = RebuildContext { ui, theme, force };
        self.mounted.state.rebuild_dirty(target, &mut context)?;
        Ok(())
    }

    pub(crate) fn route(
        &mut self,
        action: crate::RoutedComponentAction,
        services: &mut Vec<crate::ComponentServiceRequest>,
    ) -> Result<Vec<Action>, UiError> {
        let mut context = RouteContext { services };
        let output = self.mounted.state.route(&mut Some(action), &mut context)?;
        Ok(output)
    }
}

/// Creates a text view.
///
/// Text is carried as `Arc<str>` so that reconciling an unchanged label clones a
/// pointer instead of the string.
pub fn label<Action: 'static>(text: impl Into<Arc<str>>) -> AnyView<Action> {
    label_with_style(text, LabelStyle::default())
}

/// Creates a shaped text view with an optional preferred width.
pub fn label_with_width<Action: 'static>(
    text: impl Into<Arc<str>>,
    width: impl Into<Option<f32>>,
) -> AnyView<Action> {
    label_with_style(text, LabelStyle::default().width(width.into()))
}

/// Typed label presentation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabelStyle {
    /// Logical font size.
    pub font_size: f32,
    /// Semantic text color.
    pub role: ColorRole,
    /// Optional preferred width.
    pub width: Option<f32>,
}

impl LabelStyle {
    /// Creates standard body-label presentation.
    pub const fn standard() -> Self {
        Self {
            font_size: 14.0,
            role: ColorRole::Text,
            width: None,
        }
    }

    /// Selects the logical font size.
    pub const fn font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    /// Selects a semantic text color.
    pub const fn role(mut self, role: ColorRole) -> Self {
        self.role = role;
        self
    }

    /// Selects an optional preferred width.
    pub const fn width(mut self, width: Option<f32>) -> Self {
        self.width = width;
        self
    }
}

impl Default for LabelStyle {
    fn default() -> Self {
        Self::standard()
    }
}

/// Creates a text view with typed presentation.
pub fn label_with_style<Action: 'static>(
    text: impl Into<Arc<str>>,
    style: LabelStyle,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(LabelView {
            text: text.into(),
            font_size: style.font_size.max(1.0),
            role: style.role,
            width: style.width,
        }),
    }
}

/// Creates a semantic colored box.
pub fn panel<Action: 'static>(
    size: LogicalSize,
    role: ColorRole,
    semantics: Option<SemanticData>,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(BoxView {
            size,
            role,
            semantics,
            interactive: false,
        }),
    }
}

/// Typed presentation options for a button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonStyle {
    /// Semantic color treatment.
    pub variant: ButtonVariant,
    /// Preferred logical size.
    pub size: LogicalSize,
}

impl ButtonStyle {
    /// Creates standard button presentation.
    pub const fn standard() -> Self {
        Self {
            variant: ButtonVariant::Standard,
            size: LogicalSize::new(0.0, 30.0),
        }
    }

    /// Selects a semantic variant.
    pub const fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Selects the preferred logical size.
    pub const fn size(mut self, size: LogicalSize) -> Self {
        self.size = size;
        self
    }
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self::standard()
    }
}

/// Creates an activatable typed-action button.
pub fn button<Action: Clone + 'static>(text: impl Into<String>, action: Action) -> AnyView<Action> {
    button_with(text, action, ButtonStyle::standard())
}

/// Creates a button with typed presentation options.
pub fn button_with<Action: Clone + 'static>(
    text: impl Into<String>,
    action: Action,
    style: ButtonStyle,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(ButtonView {
            text: text.into(),
            action,
            variant: style.variant,
            size: style.size,
            icon: None,
            icon_size: 0.0,
            show_label: true,
        }),
    }
}

pub(crate) fn icon_button_view<Action: Clone + 'static>(
    icon: Icon,
    label: String,
    action: Action,
    style: IconButtonStyle,
) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(ButtonView {
            text: label,
            action,
            variant: style.button.variant,
            size: style.button.size,
            icon: Some(icon),
            icon_size: style.icon_size,
            show_label: style.show_label,
        }),
    }
}

/// Creates a controlled editable text field.
pub fn text_field<Action: 'static>(
    label: impl Into<String>,
    value: impl Into<String>,
    on_changed: impl Fn(String) -> Action + 'static,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(TextFieldView {
            label: label.into(),
            value: value.into(),
            on_changed: Arc::new(on_changed),
        }),
    }
}

/// Creates a controlled boolean checkbox.
pub fn checkbox<Action: 'static>(
    label: impl Into<String>,
    checked: bool,
    on_changed: impl Fn(bool) -> Action + 'static,
) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(CheckboxView {
            label: label.into(),
            checked,
            on_changed: Arc::new(on_changed),
        }),
    }
}

/// Creates a controlled horizontal numeric slider.
pub fn slider<Action: 'static>(
    label: impl Into<String>,
    value: f32,
    range: RangeInclusive<f32>,
    on_changed: impl Fn(f32) -> Action + 'static,
) -> View<Action> {
    slider_with_step(label, value, range, 1.0, on_changed)
}

/// Creates a controlled slider with an explicit keyboard step.
pub fn slider_with_step<Action: 'static>(
    label: impl Into<String>,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    on_changed: impl Fn(f32) -> Action + 'static,
) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(SliderView {
            label: label.into(),
            value,
            range,
            step: step.max(0.0),
            on_changed: Arc::new(on_changed),
        }),
    }
}

/// Mounts an independently state-owning child component.
pub fn component<C, Parent>(
    props: C::Props,
    map_effect: impl Fn(C::Effect) -> Parent + 'static,
) -> View<Parent>
where
    C: ComponentWithProps,
    Parent: 'static,
{
    AnyView {
        key: None,
        inner: Box::new(ComponentView::<C, Parent> {
            props,
            map_effect: Arc::new(map_effect),
        }),
    }
}

/// Typed action bridge supplied to custom retained-element specifications.
pub struct ActionEmitter<Action: 'static> {
    sink: Arc<dyn Fn(Action) -> Box<dyn Any>>,
}

impl<Action: 'static> Clone for ActionEmitter<Action> {
    fn clone(&self) -> Self {
        Self {
            sink: self.sink.clone(),
        }
    }
}

impl<Action: 'static> ActionEmitter<Action> {
    /// Erases one typed action for routing to its owning component.
    pub fn emit(&self, action: Action) -> Box<dyn Any> {
        (self.sink)(action)
    }
}

/// Configuration contract for a specialized retained element.
///
/// This is the escape hatch for charts, render viewports, docking surfaces,
/// and other workloads whose interaction state should remain imperative.
pub trait RetainedSpec<Action: 'static>: Clone + 'static {
    /// Concrete retained element owned by the incremental tree.
    type Element: Element;

    /// Creates newly mounted retained state.
    fn create(&self, emitter: ActionEmitter<Action>) -> Self::Element;

    /// Applies changed configuration without discarding interaction state.
    fn update(&self, element: &mut Self::Element, emitter: ActionEmitter<Action>);

    /// Reports whether retained layout, paint, or semantics may have changed.
    fn changed(&self, previous: &Self) -> bool;
}

/// Mounts a specialized retained element behind the reconciled view boundary.
pub fn retained<Action: 'static, Spec: RetainedSpec<Action>>(spec: Spec) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(RetainedView { spec }),
    }
}

struct RetainedView<Spec> {
    spec: Spec,
}

struct RetainedState<Spec: RetainedSpec<Action>, Action: 'static> {
    handle: NodeHandle<Spec::Element>,
    spec: Spec,
    marker: std::marker::PhantomData<fn() -> Action>,
}

impl<Spec: RetainedSpec<Action>, Action: 'static> MountedState<Action>
    for RetainedState<Spec, Action>
{
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl<Spec: RetainedSpec<Action>, Action: 'static> DynView<Action> for RetainedView<Spec> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let emitter = ActionEmitter {
            sink: context.action_sink.clone(),
        };
        let handle = context
            .ui
            .append(context.parent, self.spec.create(emitter))?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(RetainedState::<Spec, Action> {
                handle,
                spec: self.spec,
                marker: std::marker::PhantomData,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<RetainedState<Spec, Action>>()
            .expect("view kind and state agree");
        // The only view that cannot narrow its bits. `RetainedSpec::changed`
        // answers a single boolean, so a spec that reports change gets every
        // pass. Narrowing this needs a per-pass answer from the trait, which is
        // a public API change for every specialized element.
        let invalidation = if self.spec.changed(&state.spec) {
            Invalidation::ALL
        } else {
            Invalidation::empty()
        };
        let emitter = ActionEmitter {
            sink: context.action_sink.clone(),
        };
        context.ui.update(state.handle, invalidation, |element| {
            self.spec.update(element, emitter);
        })?;
        state.spec = self.spec;
        Ok(())
    }
}

struct ComponentView<C: ComponentWithProps, Parent: 'static> {
    props: C::Props,
    map_effect: Arc<dyn Fn(C::Effect) -> Parent>,
}

struct ComponentState<C: ComponentWithProps, Parent: 'static> {
    id: u64,
    /// Component nesting depth, used to order the dirty drain.
    depth: u32,
    parent: NodeHandle<Flex>,
    props: C::Props,
    component: C,
    child: MountedView<C::Action>,
    map_effect: Arc<dyn Fn(C::Effect) -> Parent>,
    /// Hoisted routing sink handed to every retained element in this subtree.
    ///
    /// Allocated once per mounted instance. It used to be rebuilt on every
    /// rebuild of the subtree, which also meant no two frames ever agreed on
    /// sink identity.
    sink: Arc<dyn Fn(C::Action) -> Box<dyn Any>>,
    /// Hoisted routing sink for erased service completions.
    service_route: Arc<dyn Fn(Box<dyn Any>) -> crate::ServiceAction>,
    /// Dirty set shared with the owning [`crate::ComponentRuntime`].
    dirty: DirtyHandle,
    /// [`Theme::revision`] this instance last rendered at.
    theme_revision: u64,
}

impl<C: ComponentWithProps, Parent: 'static> ComponentState<C, Parent> {
    fn action_sink(id: u64) -> Arc<dyn Fn(C::Action) -> Box<dyn Any>> {
        Arc::new(move |action| {
            Box::new(RoutedComponentAction {
                target: id,
                payload: Box::new(action),
            })
        })
    }

    fn service_sink(id: u64) -> Arc<dyn Fn(Box<dyn Any>) -> crate::ServiceAction> {
        Arc::new(move |payload| {
            Box::new(RoutedComponentAction {
                target: id,
                payload,
            })
        })
    }

    fn rebuild_child(
        &mut self,
        ui: &mut UiRoot,
        theme: &Theme,
        force: bool,
    ) -> Result<(), UiError> {
        // `component_views`: one nested component re-render. Reached only when
        // this instance's props, theme revision, or own dirty flag says its
        // output can have changed.
        ViewStats::record_component_view();
        let view = self.component.view(theme);
        self.theme_revision = theme.revision;
        let mut child_context = ViewContext {
            ui,
            parent: self.parent.id(),
            theme,
            action_sink: self.sink.clone(),
            depth: self.depth,
            dirty: &self.dirty,
            force,
            marker: std::marker::PhantomData,
        };
        if self.child.kind == view.kind() {
            view.inner
                .rebuild_counted(&mut self.child, &mut child_context)
        } else {
            child_context.ui.remove(self.child.node)?;
            self.child = view.inner.build_counted(view.key, &mut child_context)?;
            Ok(())
        }
    }

    fn reduce(
        &mut self,
        actions: Vec<C::Action>,
        context: &mut RouteContext<'_>,
    ) -> Result<Vec<Parent>, UiError> {
        if actions.is_empty() {
            return Ok(Vec::new());
        }
        let mut effects = Vec::new();
        for action in actions {
            self.component.update(
                action,
                &mut ComponentContext::new(
                    &mut effects,
                    context.services,
                    self.service_route.clone(),
                    RenderScope::nested(&self.dirty, self.depth, self.id),
                ),
            );
        }
        // Reducing does not rebuild. Anything this component's view reads may
        // have changed, so it records itself as stale and lets the runtime's
        // depth-ordered flush decide whether the parent's cascade already
        // covers it.
        self.dirty.mark(self.depth, self.id);
        Ok(effects
            .into_iter()
            .map(|effect| (self.map_effect)(effect))
            .collect())
    }
}

impl<C: ComponentWithProps, Parent: 'static> MountedState<Parent> for ComponentState<C, Parent> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn route(
        &mut self,
        action: &mut Option<RoutedComponentAction>,
        context: &mut RouteContext<'_>,
    ) -> Result<Vec<Parent>, UiError> {
        if action
            .as_ref()
            .is_some_and(|action| action.target == self.id)
        {
            let action = action.take().expect("targeted action exists");
            let action = action
                .payload
                .downcast::<C::Action>()
                .map_err(|_| UiError::new("nested component action type mismatch"))?;
            return self.reduce(vec![*action], context);
        }
        let actions = self.child.state.route(action, context)?;
        self.reduce(actions, context)
    }

    fn rebuild_dirty(
        &mut self,
        target: u64,
        context: &mut RebuildContext<'_>,
    ) -> Result<bool, UiError> {
        if self.id == target {
            self.rebuild_child(context.ui, context.theme, context.force)?;
            return Ok(true);
        }
        self.child.state.rebuild_dirty(target, context)
    }
}

impl<C: ComponentWithProps, Parent: 'static> DynView<Parent> for ComponentView<C, Parent> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    /// Records nothing before deciding whether to descend.
    ///
    /// This is the one view whose reconciliation may be a no-op. A boundary
    /// whose props, theme revision, and dirty flag all agree with the previous
    /// pass touches neither the retained tree nor its own state, so counting it
    /// as a rebuilt node would make `nodes_rebuilt` grow with the number of
    /// *untouched* sibling components — exactly the dependence update isolation
    /// exists to remove. [`Self::rebuild`] records the node itself, on the paths
    /// that really do reconcile.
    fn rebuild_counted(
        self: Box<Self>,
        mounted: &mut MountedView<Parent>,
        context: &mut ViewContext<'_, Parent>,
    ) -> Result<(), UiError> {
        self.rebuild(mounted, context)
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Parent>,
    ) -> Result<MountedView<Parent>, UiError> {
        let id = NEXT_COMPONENT_ID.fetch_add(1, Ordering::Relaxed);
        let depth = context.depth + 1;
        let parent = context.ui.append(context.parent, Flex::default())?;
        let component = C::create(&self.props);
        // `component_views`: a newly mounted nested component's first view.
        ViewStats::record_component_view();
        let view = component.view(context.theme);
        let sink = ComponentState::<C, Parent>::action_sink(id);
        let mut child_context = ViewContext {
            ui: context.ui,
            parent: parent.id(),
            theme: context.theme,
            action_sink: sink.clone(),
            depth,
            dirty: context.dirty,
            force: context.force,
            marker: std::marker::PhantomData,
        };
        let child = view.inner.build_counted(view.key, &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: parent.id(),
            state: Box::new(ComponentState::<C, Parent> {
                id,
                depth,
                parent,
                props: self.props,
                component,
                child,
                map_effect: self.map_effect,
                sink,
                service_route: ComponentState::<C, Parent>::service_sink(id),
                dirty: context.dirty.clone(),
                theme_revision: context.theme.revision,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Parent>,
        context: &mut ViewContext<'_, Parent>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<ComponentState<C, Parent>>()
            .expect("view kind and state agree");
        // The effect mapping is a fresh closure every frame and cannot be
        // compared, so it is always replaced. It is only read while routing an
        // action, never while rendering, so replacing it cannot make a skipped
        // subtree stale.
        state.map_effect = self.map_effect;
        let props_changed = state.props != self.props;
        if props_changed {
            state.component.changed(&self.props);
            state.props = self.props;
        }
        let theme_changed = state.theme_revision != context.theme.revision;
        // Taken unconditionally: a boundary refreshed here by its parent's
        // cascade must not be rebuilt a second time by the dirty drain.
        let self_dirty = state.dirty.take(state.depth, state.id);
        if !(props_changed || theme_changed || self_dirty || context.force) {
            return Ok(());
        }
        // `nodes_rebuilt`: the component boundary, counted only when it really
        // reconciles.
        ViewStats::record_node_rebuilt();
        state.rebuild_child(context.ui, context.theme, context.force)
    }
}

struct MapActionView<Child: 'static, Parent: 'static> {
    child: AnyView<Child>,
    map: Arc<dyn Fn(Child) -> Parent>,
}

struct EnabledView<Action: 'static> {
    child: AnyView<Action>,
    enabled: bool,
}

struct FrameView<Action: 'static> {
    child: AnyView<Action>,
    style: FrameStyle,
}

struct FocusScopeView<Action: 'static> {
    child: AnyView<Action>,
    active: bool,
}

struct FocusScopeState<Action: 'static> {
    child: MountedView<Action>,
    active: bool,
    previous: Option<NodeId>,
}

wrapper_mounted_state!(FocusScopeState);

impl<Action: 'static> DynView<Action> for FocusScopeView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let previous = self
            .active
            .then(|| context.ui.focused().or_else(|| context.ui.last_focused()))
            .flatten();
        let child = self
            .child
            .inner
            .build_counted(self.child.key.clone(), context)?;
        if self.active {
            context.ui.focus_first_in_subtree(child.node)?;
        }
        let node = child.node;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node,
            state: Box::new(FocusScopeState {
                child,
                active: self.active,
                previous,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<FocusScopeState<Action>>()
            .expect("view kind and state agree");
        let activating = !state.active && self.active;
        let deactivating = state.active && !self.active;
        let previous = activating
            .then(|| context.ui.focused().or_else(|| context.ui.last_focused()))
            .flatten();
        if state.child.kind == self.child.kind() {
            self.child
                .inner
                .rebuild_counted(&mut state.child, context)?;
        } else {
            context.ui.remove(state.child.node)?;
            state.child = self
                .child
                .inner
                .build_counted(self.child.key.clone(), context)?;
            mounted.node = state.child.node;
        }
        if activating {
            state.previous = previous;
            context.ui.focus_first_in_subtree(state.child.node)?;
        } else if deactivating {
            let restored = state
                .previous
                .filter(|previous| context.ui.contains(*previous))
                .is_some_and(|previous| context.ui.set_focus(Some(previous)).is_ok());
            if !restored {
                context.ui.set_focus(None)?;
            }
            state.previous = None;
        }
        state.active = self.active;
        Ok(())
    }
}

struct EscapeView<Action: Clone + 'static> {
    child: AnyView<Action>,
    action: Action,
}

struct EscapeState<Action: Clone + 'static> {
    handle: NodeHandle<KeyListener>,
    child: MountedView<Action>,
    action: ActionCell<Action>,
}

wrapper_mounted_state!(EscapeState where Action: Clone);

impl<Action: Clone + 'static> DynView<Action> for EscapeView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let action = ActionCell::new(self.action, context.action_sink.clone());
        let emit = action.emitter();
        let handle = context
            .ui
            .append(context.parent, KeyListener::on_escape(move || emit()))?;
        let mut child_context = context.reborrow(handle.id());
        let child = self
            .child
            .inner
            .build_counted(self.child.key.clone(), &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(EscapeState {
                handle,
                child,
                action,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<EscapeState<Action>>()
            .expect("view kind and state agree");
        // Writing the cell replaces the dismissal action without reinstalling a
        // listener closure, which is why no retained update happens here at all.
        state.action.update(self.action, &context.action_sink);
        let mut child_context = context.reborrow(state.handle.id());
        if state.child.kind == self.child.kind() {
            self.child
                .inner
                .rebuild_counted(&mut state.child, &mut child_context)?;
        } else {
            child_context.ui.remove(state.child.node)?;
            state.child = self
                .child
                .inner
                .build_counted(self.child.key.clone(), &mut child_context)?;
        }
        Ok(())
    }
}

struct CommandNavigationView<Action: Clone + 'static> {
    child: AnyView<Action>,
    previous: Action,
    next: Action,
    submit: Action,
}

struct CommandNavigationState<Action: Clone + 'static> {
    handle: NodeHandle<KeyListener>,
    child: MountedView<Action>,
    previous: ActionCell<Action>,
    next: ActionCell<Action>,
    submit: ActionCell<Action>,
}

wrapper_mounted_state!(CommandNavigationState where Action: Clone);

impl<Action: Clone + 'static> DynView<Action> for CommandNavigationView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let previous = ActionCell::new(self.previous, context.action_sink.clone());
        let next = ActionCell::new(self.next, context.action_sink.clone());
        let submit = ActionCell::new(self.submit, context.action_sink.clone());
        let emit_previous = previous.emitter();
        let emit_next = next.emitter();
        let emit_submit = submit.emitter();
        let handle = context.ui.append(
            context.parent,
            KeyListener::command_navigation(
                move || emit_previous(),
                move || emit_next(),
                move || emit_submit(),
            ),
        )?;
        let mut child_context = context.reborrow(handle.id());
        let child = self
            .child
            .inner
            .build_counted(self.child.key.clone(), &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(CommandNavigationState {
                handle,
                child,
                previous,
                next,
                submit,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<CommandNavigationState<Action>>()
            .expect("view kind and state agree");
        state.previous.update(self.previous, &context.action_sink);
        state.next.update(self.next, &context.action_sink);
        state.submit.update(self.submit, &context.action_sink);
        let mut child_context = context.reborrow(state.handle.id());
        if state.child.kind == self.child.kind() {
            self.child
                .inner
                .rebuild_counted(&mut state.child, &mut child_context)?;
        } else {
            child_context.ui.remove(state.child.node)?;
            state.child = self
                .child
                .inner
                .build_counted(self.child.key.clone(), &mut child_context)?;
        }
        Ok(())
    }
}

struct AlignView<Action: 'static> {
    child: AnyView<Action>,
    alignment: Alignment,
    padding: Space,
}

struct AlignState<Action: 'static> {
    handle: NodeHandle<Align>,
    child: MountedView<Action>,
    alignment: Alignment,
    padding: f32,
}

wrapper_mounted_state!(AlignState);

impl<Action: 'static> DynView<Action> for AlignView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let padding = context.theme.space(self.padding);
        let handle = context.ui.append(
            context.parent,
            Align {
                alignment: self.alignment,
                padding,
            },
        )?;
        let mut child_context = context.reborrow(handle.id());
        let child = self
            .child
            .inner
            .build_counted(self.child.key.clone(), &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(AlignState {
                handle,
                child,
                alignment: self.alignment,
                padding,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<AlignState<Action>>()
            .expect("view kind and state agree");
        let padding = context.theme.space(self.padding);
        if state.alignment != self.alignment || state.padding != padding {
            context
                .ui
                .update(state.handle, Invalidation::LAYOUT_ALL, |align| {
                    align.alignment = self.alignment;
                    align.padding = padding;
                })?;
        }
        let mut child_context = context.reborrow(state.handle.id());
        if state.child.kind == self.child.kind() {
            self.child
                .inner
                .rebuild_counted(&mut state.child, &mut child_context)?;
        } else {
            child_context.ui.remove(state.child.node)?;
            state.child = self
                .child
                .inner
                .build_counted(self.child.key.clone(), &mut child_context)?;
        }
        state.alignment = self.alignment;
        state.padding = padding;
        Ok(())
    }
}

struct FrameState<Action: 'static> {
    handle: NodeHandle<Frame>,
    child: MountedView<Action>,
    style: FrameStyle,
}

wrapper_mounted_state!(FrameState);

impl<Action: 'static> DynView<Action> for FrameView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let handle = context.ui.append(
            context.parent,
            Frame {
                width: self.style.width,
                height: self.style.height,
                min: self.style.min,
                max: self.style.max,
                grow: self.style.grow,
            },
        )?;
        let mut child_context = context.reborrow(handle.id());
        let child = self
            .child
            .inner
            .build_counted(self.child.key.clone(), &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(FrameState {
                handle,
                child,
                style: self.style,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<FrameState<Action>>()
            .expect("view kind and state agree");
        if state.style != self.style {
            // Every field of a frame is a layout input, so `LAYOUT_ALL` is the
            // exact answer. Normalization mirrors what `set_frame` guaranteed.
            let style = self.style;
            context
                .ui
                .update(state.handle, Invalidation::LAYOUT_ALL, |frame| {
                    frame.width = style.width.map(|value| value.max(0.0));
                    frame.height = style.height.map(|value| value.max(0.0));
                    frame.min =
                        LogicalSize::new(style.min.width.max(0.0), style.min.height.max(0.0));
                    frame.max = style.max.map(|size| {
                        LogicalSize::new(
                            size.width.max(frame.min.width),
                            size.height.max(frame.min.height),
                        )
                    });
                    frame.grow = style.grow.max(0.0);
                })?;
        }
        let mut child_context = context.reborrow(state.handle.id());
        if state.child.kind == self.child.kind() {
            self.child
                .inner
                .rebuild_counted(&mut state.child, &mut child_context)?;
        } else {
            child_context.ui.remove(state.child.node)?;
            state.child = self
                .child
                .inner
                .build_counted(self.child.key.clone(), &mut child_context)?;
        }
        state.style = self.style;
        Ok(())
    }
}

struct VisibleView<Action: 'static> {
    child: AnyView<Action>,
    visible: bool,
}

struct VisibleState<Action: 'static> {
    child: MountedView<Action>,
    visible: bool,
}

wrapper_mounted_state!(VisibleState);

impl<Action: 'static> DynView<Action> for VisibleView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let child = self
            .child
            .inner
            .build_counted(self.child.key.clone(), context)?;
        context.ui.set_visible(child.node, self.visible)?;
        let node = child.node;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node,
            state: Box::new(VisibleState {
                child,
                visible: self.visible,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<VisibleState<Action>>()
            .expect("view kind and state agree");
        if state.child.kind == self.child.kind() {
            self.child
                .inner
                .rebuild_counted(&mut state.child, context)?;
        } else {
            context.ui.remove(state.child.node)?;
            state.child = self
                .child
                .inner
                .build_counted(self.child.key.clone(), context)?;
            mounted.node = state.child.node;
        }
        if state.visible != self.visible {
            context.ui.set_visible(state.child.node, self.visible)?;
        }
        state.visible = self.visible;
        Ok(())
    }
}

struct EnabledState<Action: 'static> {
    child: MountedView<Action>,
    enabled: bool,
}

wrapper_mounted_state!(EnabledState);

impl<Action: 'static> DynView<Action> for EnabledView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let child = self
            .child
            .inner
            .build_counted(self.child.key.clone(), context)?;
        context.ui.set_enabled(child.node, self.enabled)?;
        let node = child.node;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node,
            state: Box::new(EnabledState {
                child,
                enabled: self.enabled,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<EnabledState<Action>>()
            .expect("view kind and state agree");
        if state.child.kind == self.child.kind() {
            self.child
                .inner
                .rebuild_counted(&mut state.child, context)?;
        } else {
            context.ui.remove(state.child.node)?;
            state.child = self
                .child
                .inner
                .build_counted(self.child.key.clone(), context)?;
            mounted.node = state.child.node;
        }
        if state.enabled != self.enabled {
            context.ui.set_enabled(state.child.node, self.enabled)?;
        }
        state.enabled = self.enabled;
        Ok(())
    }
}

/// Mutable inputs of a composed action sink.
///
/// [`AnyView::map_action`] takes a fresh closure every frame, so the mapping's
/// identity can never be compared and a naive rebuild has to allocate a new
/// composed sink each pass. Every retained element below the wrapper then sees a
/// different sink identity on every frame, which is invisible today but makes
/// any future memo keyed on sink identity miss unconditionally. Routing through
/// this cell keeps the composed sink allocated exactly once at mount while still
/// calling the newest mapping.
struct MapActionCell<Child: 'static, Parent: 'static> {
    map: Arc<dyn Fn(Child) -> Parent>,
    parent_sink: Arc<dyn Fn(Parent) -> Box<dyn Any>>,
}

impl<Child: 'static, Parent: 'static> MapActionCell<Child, Parent> {
    /// Builds the single composed sink that reads through `cell` forever.
    fn compose(cell: &Rc<RefCell<Self>>) -> Arc<dyn Fn(Child) -> Box<dyn Any>> {
        let cell = cell.clone();
        Arc::new(move |action| {
            // The closures are cloned out before being called so that a user
            // mapping can never observe this cell mid-borrow.
            let (map, parent_sink) = {
                let cell = cell.borrow();
                (cell.map.clone(), cell.parent_sink.clone())
            };
            parent_sink(map(action))
        })
    }
}

struct MapActionState<Child: 'static, Parent: 'static> {
    child: MountedView<Child>,
    cell: Rc<RefCell<MapActionCell<Child, Parent>>>,
    /// Sink handed to the child subtree. Allocated at mount and never replaced.
    composed: Arc<dyn Fn(Child) -> Box<dyn Any>>,
}

impl<Child: 'static, Parent: 'static> MountedState<Parent> for MapActionState<Child, Parent> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn route(
        &mut self,
        action: &mut Option<crate::RoutedComponentAction>,
        context: &mut RouteContext<'_>,
    ) -> Result<Vec<Parent>, UiError> {
        let actions = self.child.state.route(action, context)?;
        let map = self.cell.borrow().map.clone();
        Ok(actions.into_iter().map(|action| map(action)).collect())
    }

    fn rebuild_dirty(
        &mut self,
        target: u64,
        context: &mut RebuildContext<'_>,
    ) -> Result<bool, UiError> {
        self.child.state.rebuild_dirty(target, context)
    }
}

impl<Child: 'static, Parent: 'static> DynView<Parent> for MapActionView<Child, Parent> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Parent>,
    ) -> Result<MountedView<Parent>, UiError> {
        let cell = Rc::new(RefCell::new(MapActionCell {
            map: self.map,
            parent_sink: context.action_sink.clone(),
        }));
        let composed = MapActionCell::compose(&cell);
        let mut child_context = ViewContext {
            ui: context.ui,
            parent: context.parent,
            theme: context.theme,
            action_sink: composed.clone(),
            depth: context.depth,
            dirty: context.dirty,
            force: context.force,
            marker: std::marker::PhantomData,
        };
        let child = self
            .child
            .inner
            .build_counted(self.child.key.clone(), &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: child.node,
            state: Box::new(MapActionState {
                child,
                cell,
                composed,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Parent>,
        context: &mut ViewContext<'_, Parent>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<MapActionState<Child, Parent>>()
            .expect("view kind and state agree");
        {
            let mut cell = state.cell.borrow_mut();
            cell.map = self.map;
            if !Arc::ptr_eq(&cell.parent_sink, &context.action_sink) {
                cell.parent_sink = context.action_sink.clone();
            }
        }
        let mut child_context = ViewContext {
            ui: context.ui,
            parent: context.parent,
            theme: context.theme,
            action_sink: state.composed.clone(),
            depth: context.depth,
            dirty: context.dirty,
            force: context.force,
            marker: std::marker::PhantomData,
        };
        if state.child.kind == self.child.kind() {
            self.child
                .inner
                .rebuild_counted(&mut state.child, &mut child_context)?;
        } else {
            child_context.ui.remove(state.child.node)?;
            state.child = self
                .child
                .inner
                .build_counted(self.child.key.clone(), &mut child_context)?;
            mounted.node = state.child.node;
        }
        Ok(())
    }
}

/// Creates a vertical flex view.
pub fn column<Action: 'static>(children: impl IntoChildren<Action>) -> AnyView<Action> {
    column_with(ContainerStyle::default(), children)
}

/// Creates a horizontal flex view.
pub fn row<Action: 'static>(children: impl IntoChildren<Action>) -> AnyView<Action> {
    row_with(ContainerStyle::default(), children)
}

/// Typed presentation options for row and column containers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContainerStyle {
    /// Space between children.
    pub gap: Space,
    /// Uniform inset around children.
    pub padding: Space,
    /// Optional semantic background fill.
    pub background: Option<ColorRole>,
}

impl Default for ContainerStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerStyle {
    /// Creates the default compact container style.
    pub const fn new() -> Self {
        Self {
            gap: Space::Sm,
            padding: Space::None,
            background: None,
        }
    }

    /// Selects semantic child spacing.
    pub const fn gap(mut self, gap: Space) -> Self {
        self.gap = gap;
        self
    }

    /// Selects uniform semantic padding.
    pub const fn padding(mut self, padding: Space) -> Self {
        self.padding = padding;
        self
    }

    /// Selects a semantic background role.
    pub const fn background(mut self, background: ColorRole) -> Self {
        self.background = Some(background);
        self
    }
}

/// Creates a styled vertical container.
pub fn column_with<Action: 'static>(
    style: ContainerStyle,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    container(Axis::Vertical, style, children)
}

/// Creates a styled horizontal container.
pub fn row_with<Action: 'static>(
    style: ContainerStyle,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    container(Axis::Horizontal, style, children)
}

/// Creates a fixed empty layout region.
pub fn spacer<Action: 'static>(size: LogicalSize) -> AnyView<Action> {
    panel(size, ColorRole::Transparent, None)
}

/// Typed presentation options for an overlay stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackStyle {
    /// Uniform inset around overlaid children.
    pub padding: Space,
    /// Optional semantic background fill.
    pub background: Option<ColorRole>,
}

impl Default for StackStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl StackStyle {
    /// Creates an undecorated stack.
    pub const fn new() -> Self {
        Self {
            padding: Space::None,
            background: None,
        }
    }

    /// Selects uniform semantic padding.
    pub const fn padding(mut self, padding: Space) -> Self {
        self.padding = padding;
        self
    }

    /// Selects a semantic background role.
    pub const fn background(mut self, background: ColorRole) -> Self {
        self.background = Some(background);
        self
    }
}

/// Overlays children in paint order at one shared origin.
pub fn stack<Action: 'static>(children: impl IntoChildren<Action>) -> AnyView<Action> {
    stack_with(StackStyle::new(), children)
}

/// Creates a styled overlay stack.
pub fn stack_with<Action: 'static>(
    style: StackStyle,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(StackView {
            padding: style.padding,
            background: style.background,
            children: children.into_children(),
        }),
    }
}

/// Creates a clipped viewport with retained wheel offset.
pub fn scroll<Action: 'static>(
    axis: ScrollAxis,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    scroll_at(axis, LogicalPoint::ZERO, children)
}

/// Creates a clipped viewport with an explicit initial or controlled offset.
pub fn scroll_at<Action: 'static>(
    axis: ScrollAxis,
    offset: LogicalPoint,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(ScrollView {
            axis,
            offset,
            children: children.into_children(),
        }),
    }
}

fn container<Action: 'static>(
    axis: Axis,
    style: ContainerStyle,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(FlexView {
            axis,
            gap: style.gap,
            padding: style.padding,
            background: style.background,
            children: children.into_children(),
        }),
    }
}

/// Creates a flex view with semantic spacing.
pub fn flex<Action: 'static>(
    axis: Axis,
    gap: Space,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    container(axis, ContainerStyle::new().gap(gap), children)
}

/// Creates a two-child pane with a dedicated draggable divider.
pub fn split_pane<Action: 'static>(
    axis: Axis,
    ratio: f32,
    first: View<Action>,
    second: View<Action>,
    on_resize: impl Fn(f32) -> Action + 'static,
) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(SplitPaneView {
            axis,
            ratio: ratio.clamp(0.05, 0.95),
            on_resize: Arc::new(on_resize),
            children: vec![first, second],
        }),
    }
}

/// Mutable inputs of one retained control's change callback.
struct MapTarget<Input: 'static, Action: 'static> {
    map: Arc<dyn Fn(Input) -> Action>,
    sink: Arc<dyn Fn(Action) -> Box<dyn Any>>,
}

/// Stable indirection behind a control's change callback.
///
/// A retained control owns a boxed erased factory. Reinstalling it on every
/// rebuild allocated a closure and a box per control per frame for a value that
/// virtually never changes. Instead the element captures a clone of this cell
/// once, at mount, and reads the current mapping through it; a rebuild only
/// writes the cell.
struct MapCell<Input: 'static, Action: 'static>(Rc<RefCell<MapTarget<Input, Action>>>);

impl<Input: 'static, Action: 'static> MapCell<Input, Action> {
    fn new(map: Arc<dyn Fn(Input) -> Action>, sink: Arc<dyn Fn(Action) -> Box<dyn Any>>) -> Self {
        Self(Rc::new(RefCell::new(MapTarget { map, sink })))
    }

    /// Returns the closure installed into the retained element exactly once.
    fn emitter(&self) -> impl Fn(Input) -> Box<dyn Any> + use<Input, Action> {
        let cell = self.0.clone();
        move |input| {
            // Cloned out before being called so a user mapping can never observe
            // this cell mid-borrow.
            let (map, sink) = {
                let target = cell.borrow();
                (target.map.clone(), target.sink.clone())
            };
            sink(map(input))
        }
    }

    /// Points the cell at this frame's mapping without touching the element.
    fn update(
        &self,
        map: Arc<dyn Fn(Input) -> Action>,
        sink: &Arc<dyn Fn(Action) -> Box<dyn Any>>,
    ) {
        let mut target = self.0.borrow_mut();
        target.map = map;
        if !Arc::ptr_eq(&target.sink, sink) {
            target.sink = sink.clone();
        }
    }
}

/// Mutable inputs of one retained control's activation action.
struct ActionTarget<Action: 'static> {
    action: Action,
    sink: Arc<dyn Fn(Action) -> Box<dyn Any>>,
}

/// Stable indirection behind an activation action carried by value.
struct ActionCell<Action: Clone + 'static>(Rc<RefCell<ActionTarget<Action>>>);

impl<Action: Clone + 'static> ActionCell<Action> {
    fn new(action: Action, sink: Arc<dyn Fn(Action) -> Box<dyn Any>>) -> Self {
        Self(Rc::new(RefCell::new(ActionTarget { action, sink })))
    }

    /// Returns the closure installed into the retained element exactly once.
    fn emitter(&self) -> impl Fn() -> Box<dyn Any> + use<Action> {
        let cell = self.0.clone();
        move || {
            let (action, sink) = {
                let target = cell.borrow();
                (target.action.clone(), target.sink.clone())
            };
            sink(action)
        }
    }

    /// Points the cell at this frame's action without touching the element.
    fn update(&self, action: Action, sink: &Arc<dyn Fn(Action) -> Box<dyn Any>>) {
        let mut target = self.0.borrow_mut();
        target.action = action;
        if !Arc::ptr_eq(&target.sink, sink) {
            target.sink = sink.clone();
        }
    }
}

struct LabelView {
    text: Arc<str>,
    font_size: f32,
    role: ColorRole,
    width: Option<f32>,
}

struct LabelState {
    handle: NodeHandle<Label>,
    text: Arc<str>,
    font_size: f32,
    color: Color,
    width: Option<f32>,
}

leaf_mounted_state!(LabelState);

impl<Action: 'static> DynView<Action> for LabelView {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let color = context.theme.color(self.role);
        let mut label = Label::new(&*self.text)
            .with_font_size(self.font_size)
            .with_color(color);
        label.set_width(self.width);
        let handle = context.ui.append(context.parent, label)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(LabelState {
                handle,
                text: self.text,
                font_size: self.font_size,
                color,
                width: self.width,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<LabelState>()
            .expect("view kind and state agree");
        let color = context.theme.color(self.role);
        if state.text == self.text
            && state.font_size == self.font_size
            && state.width == self.width
            && state.color == color
        {
            return Ok(());
        }
        // `LAYOUT_ALL` is exact for a label rather than a fallback: the shaper
        // bakes the brush into every glyph run, so even a color-only change has
        // to re-shape, and re-shaping happens in layout. The one thing that is
        // *not* exact is the comparison — `Label::preferred_width` is private,
        // so the retained width cannot be read back and is mirrored here
        // instead.
        //
        // The engine's own per-element shaping memo keeps a redundant re-shape
        // cheap; this cannot become narrower until paint takes a text brush.
        let text = String::from(&*self.text);
        context
            .ui
            .update(state.handle, Invalidation::LAYOUT_ALL, |label| {
                label.text = text;
                label.font_size = self.font_size;
                label.color = Some(color);
                label.set_width(self.width);
            })?;
        state.text = self.text;
        state.font_size = self.font_size;
        state.color = color;
        state.width = self.width;
        Ok(())
    }
}

struct BoxView {
    size: LogicalSize,
    role: ColorRole,
    semantics: Option<SemanticData>,
    interactive: bool,
}

struct BoxState {
    handle: NodeHandle<BoxElement>,
    size: LogicalSize,
    color: Color,
    semantics: Option<SemanticData>,
    interactive: bool,
}

leaf_mounted_state!(BoxState);

impl<Action: 'static> DynView<Action> for BoxView {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let color = context.theme.color(self.role);
        let handle = context.ui.append(
            context.parent,
            BoxElement {
                size: self.size,
                color,
                semantics: self.semantics.clone(),
                interactive: self.interactive,
            },
        )?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(BoxState {
                handle,
                size: self.size,
                color,
                semantics: self.semantics,
                interactive: self.interactive,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<BoxState>()
            .expect("view kind and state agree");
        let color = context.theme.color(self.role);
        let mut invalidation = Invalidation::empty();
        if state.size != self.size {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.color != color {
            invalidation |= Invalidation::PAINT;
        }
        if state.semantics != self.semantics {
            invalidation |= Invalidation::ACCESSIBILITY;
        }
        if state.interactive != self.interactive {
            invalidation |= Invalidation::HIT_TEST;
        }
        if invalidation.is_empty() {
            return Ok(());
        }
        // These bits were already computed correctly and then discarded by
        // `set_box`, which asks for `LAYOUT_ALL` unconditionally. A selection
        // mark that only changes color now repaints instead of relaying out.
        let semantics = self.semantics.clone();
        let interactive = self.interactive;
        let size = self.size;
        context.ui.update(state.handle, invalidation, |element| {
            element.size = size;
            element.color = color;
            element.semantics = semantics;
            element.interactive = interactive;
        })?;
        state.size = self.size;
        state.color = color;
        state.semantics = self.semantics;
        state.interactive = self.interactive;
        Ok(())
    }
}

struct ButtonView<Action> {
    text: String,
    action: Action,
    variant: ButtonVariant,
    size: LogicalSize,
    icon: Option<Icon>,
    icon_size: f32,
    show_label: bool,
}

struct ButtonState<Action: Clone + 'static> {
    handle: NodeHandle<Button>,
    text: String,
    variant: ButtonVariant,
    size: LogicalSize,
    colors: (Color, Color),
    icon: Option<(u64, LogicalSize, f32, astrelis_paint::FillRule)>,
    show_label: bool,
    action: ActionCell<Action>,
}

impl<Action: Clone + 'static> MountedState<Action> for ButtonState<Action> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl<Action: Clone + 'static> DynView<Action> for ButtonView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let colors = context.theme.button(self.variant);
        let action = ActionCell::new(self.action, context.action_sink.clone());
        let emit = action.emitter();
        let icon_state = self.icon.as_ref().map(|icon| {
            (
                icon.path.cache_id(),
                icon.view_box,
                if self.icon_size.is_finite() {
                    self.icon_size.max(1.0)
                } else {
                    16.0
                },
                icon.fill_rule,
            )
        });
        let mut button = Button::with_action_factory(
            self.text.clone(),
            self.size,
            colors.0,
            colors.1,
            move || emit(),
        )
        .with_label_visible(self.show_label);
        if let (Some(icon), Some((_, _, icon_size, fill_rule))) = (&self.icon, icon_state) {
            button = button.with_icon(
                ButtonIcon::new(icon.path.clone(), icon.view_box, icon_size)
                    .with_fill_rule(fill_rule),
            );
        }
        let handle = context.ui.append(context.parent, button)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(ButtonState {
                handle,
                text: self.text,
                variant: self.variant,
                size: self.size,
                colors,
                icon: icon_state,
                show_label: self.show_label,
                action,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<ButtonState<Action>>()
            .expect("view kind and state agree");
        let colors = context.theme.button(self.variant);
        let icon_state = self.icon.as_ref().map(|icon| {
            (
                icon.path.cache_id(),
                icon.view_box,
                if self.icon_size.is_finite() {
                    self.icon_size.max(1.0)
                } else {
                    16.0
                },
                icon.fill_rule,
            )
        });
        // The activation action is written into the cell the retained element
        // already reads through, so a changed action no longer reinstalls a
        // boxed closure.
        state.action.update(self.action, &context.action_sink);
        // Label, size, and icon geometry feed layout; the two fills only repaint.
        let mut invalidation = Invalidation::empty();
        if state.size != self.size
            || state.text != self.text
            || state.icon != icon_state
            || state.show_label != self.show_label
        {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.colors != colors {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            let text = self.text.clone();
            let size = self.size;
            let show_label = self.show_label;
            let icon = (state.icon != icon_state).then(|| {
                self.icon.as_ref().map(|icon| {
                    ButtonIcon::new(
                        icon.path.clone(),
                        icon.view_box,
                        icon_state.map(|(_, _, size, _)| size).unwrap_or(16.0),
                    )
                    .with_fill_rule(icon.fill_rule)
                })
            });
            context.ui.update(state.handle, invalidation, |button| {
                button.label = text;
                button.size = size;
                button.color = colors.0;
                button.pressed_color = colors.1;
                button.show_label = show_label;
                if let Some(icon) = icon {
                    button.icon = icon;
                }
            })?;
        }
        state.text = self.text;
        state.variant = self.variant;
        state.size = self.size;
        state.colors = colors;
        state.icon = icon_state;
        state.show_label = self.show_label;
        Ok(())
    }
}

struct TextFieldView<Action: 'static> {
    label: String,
    value: String,
    on_changed: Arc<dyn Fn(String) -> Action>,
}

struct TextFieldState<Action: 'static> {
    handle: NodeHandle<TextField>,
    label: String,
    value: String,
    colors: (Color, Color),
    changed: MapCell<String, Action>,
}

impl<Action: 'static> MountedState<Action> for TextFieldState<Action> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl<Action: 'static> DynView<Action> for TextFieldView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let colors = (
            context.theme.color(ColorRole::Text),
            context.theme.color(ColorRole::Surface),
        );
        let changed = MapCell::new(self.on_changed, context.action_sink.clone());
        let emit = changed.emitter();
        let mut field = TextField::new(self.label.clone(), self.value.clone())
            .on_changed_factory(move |value| emit(value));
        field.text_color = colors.0;
        field.background = colors.1;
        let handle = context.ui.append(context.parent, field)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(TextFieldState {
                handle,
                label: self.label,
                value: self.value,
                colors,
                changed,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<TextFieldState<Action>>()
            .expect("view kind and state agree");
        let colors = (
            context.theme.color(ColorRole::Text),
            context.theme.color(ColorRole::Surface),
        );
        state.changed.update(self.on_changed, &context.action_sink);
        // Value, placeholder label, and glyph color all reach the shaper, which
        // runs in layout. Only the field's background is paint-only.
        let mut invalidation = Invalidation::empty();
        if state.value != self.value || state.label != self.label || state.colors.0 != colors.0 {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.colors.1 != colors.1 {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            let label = self.label.clone();
            let value = self.value.clone();
            context.ui.update(state.handle, invalidation, |field| {
                field.label = label;
                field.set_text(value);
                field.text_color = colors.0;
                field.background = colors.1;
            })?;
        }
        state.label = self.label;
        state.value = self.value;
        state.colors = colors;
        Ok(())
    }
}

struct CheckboxView<Action: 'static> {
    label: String,
    checked: bool,
    on_changed: Arc<dyn Fn(bool) -> Action>,
}

struct CheckboxState<Action: 'static> {
    handle: NodeHandle<Checkbox>,
    label: String,
    checked: bool,
    colors: (Color, Color, Color),
    changed: MapCell<bool, Action>,
}

impl<Action: 'static> MountedState<Action> for CheckboxState<Action> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl<Action: 'static> DynView<Action> for CheckboxView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let colors = (
            context.theme.color(ColorRole::Text),
            context.theme.color(ColorRole::Muted),
            context.theme.color(ColorRole::Accent),
        );
        let changed = MapCell::new(self.on_changed, context.action_sink.clone());
        let emit = changed.emitter();
        let mut checkbox = Checkbox::new(self.label.clone(), self.checked, move |checked| {
            emit(checked)
        });
        checkbox.text_color = colors.0;
        checkbox.outline_color = colors.1;
        checkbox.accent_color = colors.2;
        let handle = context.ui.append(context.parent, checkbox)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(CheckboxState {
                handle,
                label: self.label,
                checked: self.checked,
                colors,
                changed,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<CheckboxState<Action>>()
            .expect("view kind and state agree");
        let colors = (
            context.theme.color(ColorRole::Text),
            context.theme.color(ColorRole::Muted),
            context.theme.color(ColorRole::Accent),
        );
        state.changed.update(self.on_changed, &context.action_sink);
        // The label and its glyph color reach the shaper, which runs in layout.
        // The checked state changes the indicator fill and the accessible value;
        // the outline and accent fills are paint-only.
        let mut invalidation = Invalidation::empty();
        if state.label != self.label || state.colors.0 != colors.0 {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.checked != self.checked {
            invalidation |= Invalidation::PAINT | Invalidation::ACCESSIBILITY;
        }
        if state.colors.1 != colors.1 || state.colors.2 != colors.2 {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            let label = self.label.clone();
            let checked = self.checked;
            context.ui.update(state.handle, invalidation, |checkbox| {
                checkbox.label = label;
                checkbox.checked = checked;
                checkbox.text_color = colors.0;
                checkbox.outline_color = colors.1;
                checkbox.accent_color = colors.2;
            })?;
        }
        state.label = self.label;
        state.checked = self.checked;
        state.colors = colors;
        Ok(())
    }
}

struct SliderView<Action: 'static> {
    label: String,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    on_changed: Arc<dyn Fn(f32) -> Action>,
}

struct SliderState<Action: 'static> {
    handle: NodeHandle<Slider>,
    label: String,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    colors: (Color, Color),
    changed: MapCell<f32, Action>,
}

impl<Action: 'static> MountedState<Action> for SliderState<Action> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl<Action: 'static> DynView<Action> for SliderView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let colors = (
            context.theme.color(ColorRole::Muted),
            context.theme.color(ColorRole::Accent),
        );
        let changed = MapCell::new(self.on_changed, context.action_sink.clone());
        let emit = changed.emitter();
        let mut slider = Slider::new(
            self.label.clone(),
            self.value,
            self.range.clone(),
            move |value| emit(value),
        );
        slider.step = self.step;
        slider.track_color = colors.0;
        slider.accent_color = colors.1;
        let handle = context.ui.append(context.parent, slider)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(SliderState {
                handle,
                label: self.label,
                value: self.value,
                range: self.range,
                step: self.step,
                colors,
                changed,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<SliderState<Action>>()
            .expect("view kind and state agree");
        let colors = (
            context.theme.color(ColorRole::Muted),
            context.theme.color(ColorRole::Accent),
        );
        state.changed.update(self.on_changed, &context.action_sink);
        // A slider's size is fixed and its label is never painted, so nothing
        // here relayouts. The label is accessibility-only; value and range move
        // the thumb and the reported value; the step is neither painted nor
        // exposed, so changing it invalidates nothing.
        let mut invalidation = Invalidation::empty();
        if state.label != self.label {
            invalidation |= Invalidation::ACCESSIBILITY;
        }
        if state.value != self.value || state.range != self.range {
            invalidation |= Invalidation::PAINT | Invalidation::ACCESSIBILITY;
        }
        if state.colors != colors {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() || state.step != self.step {
            let label = self.label.clone();
            let range = self.range.clone();
            let value = self.value;
            let step = self.step;
            context.ui.update(state.handle, invalidation, |slider| {
                // Normalization and clamping mirror `Slider::new`, which is what
                // the retained element guarantees about these fields.
                let start = (*range.start()).min(*range.end());
                let end = (*range.start()).max(*range.end());
                slider.label = label;
                slider.range = start..=end;
                slider.value = value.clamp(start, end);
                slider.step = step.max(0.0);
                slider.track_color = colors.0;
                slider.accent_color = colors.1;
            })?;
        }
        state.label = self.label;
        state.value = self.value;
        state.range = self.range;
        state.step = self.step;
        state.colors = colors;
        Ok(())
    }
}

struct FlexView<Action: 'static> {
    axis: Axis,
    gap: Space,
    padding: Space,
    background: Option<ColorRole>,
    children: Vec<AnyView<Action>>,
}

struct SplitPaneView<Action: 'static> {
    axis: Axis,
    ratio: f32,
    on_resize: Arc<dyn Fn(f32) -> Action>,
    children: Vec<AnyView<Action>>,
}

struct SplitPaneState<Action: 'static> {
    handle: NodeHandle<SplitPane>,
    axis: Axis,
    ratio: f32,
    resize: MapCell<f32, Action>,
    children: MountedChildren<Action>,
}

container_mounted_state!(SplitPaneState);

impl<Action: 'static> DynView<Action> for SplitPaneView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let resize = MapCell::new(self.on_resize, context.action_sink.clone());
        let emit = resize.emitter();
        let handle = context.ui.append(
            context.parent,
            SplitPane::new(self.axis, self.ratio, move |ratio| emit(ratio)),
        )?;
        let mut child_context = context.reborrow(handle.id());
        let mut children = MountedChildren::new();
        children.build(self.children, &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(SplitPaneState {
                handle,
                axis: self.axis,
                ratio: self.ratio,
                resize,
                children,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<SplitPaneState<Action>>()
            .expect("view kind and state agree");
        state.resize.update(self.on_resize, &context.action_sink);
        // Guarded like every other container. This was the one view that asked
        // for `LAYOUT_ALL` on every pass whether or not the divider moved.
        if state.axis != self.axis || state.ratio != self.ratio {
            context
                .ui
                .update(state.handle, Invalidation::LAYOUT_ALL, |split| {
                    split.axis = self.axis;
                    split.ratio = self.ratio;
                })?;
            state.axis = self.axis;
            state.ratio = self.ratio;
        }
        let mut child_context = context.reborrow(state.handle.id());
        state.children.reconcile(self.children, &mut child_context)
    }
}

struct StackView<Action: 'static> {
    padding: Space,
    background: Option<ColorRole>,
    children: Vec<AnyView<Action>>,
}

struct ScrollView<Action: 'static> {
    axis: ScrollAxis,
    offset: LogicalPoint,
    children: Vec<AnyView<Action>>,
}

struct ScrollState<Action: 'static> {
    handle: NodeHandle<Scroll>,
    axis: ScrollAxis,
    requested_offset: LogicalPoint,
    children: MountedChildren<Action>,
}

container_mounted_state!(ScrollState);

impl<Action: 'static> DynView<Action> for ScrollView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let handle = context.ui.append(context.parent, Scroll::new(self.axis))?;
        // Both fields feed `Scroll::layout`, which measures children against the
        // axis and places them at the offset.
        context
            .ui
            .update(handle, Invalidation::LAYOUT_ALL, |scroll| {
                scroll.axis = self.axis;
                scroll.offset = self.offset;
            })?;
        let mut child_context = context.reborrow(handle.id());
        let mut children = MountedChildren::new();
        children.build(self.children, &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(ScrollState {
                handle,
                axis: self.axis,
                requested_offset: self.offset,
                children,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<ScrollState<Action>>()
            .expect("view kind and state agree");
        if state.axis != self.axis || state.requested_offset != self.offset {
            context
                .ui
                .update(state.handle, Invalidation::LAYOUT_ALL, |scroll| {
                    scroll.axis = self.axis;
                    scroll.offset = self.offset;
                })?;
            state.axis = self.axis;
            state.requested_offset = self.offset;
        }
        let mut child_context = context.reborrow(state.handle.id());
        state.children.reconcile(self.children, &mut child_context)
    }
}

struct StackState<Action: 'static> {
    handle: NodeHandle<Stack>,
    padding: f32,
    background: Option<Color>,
    children: MountedChildren<Action>,
}

container_mounted_state!(StackState);

impl<Action: 'static> DynView<Action> for StackView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let padding = context.theme.space(self.padding);
        let background = self.background.map(|role| context.theme.color(role));
        let handle = context.ui.append(
            context.parent,
            Stack {
                padding,
                background,
            },
        )?;
        let mut child_context = context.reborrow(handle.id());
        let mut children = MountedChildren::new();
        children.build(self.children, &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(StackState {
                handle,
                padding,
                background,
                children,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<StackState<Action>>()
            .expect("view kind and state agree");
        let padding = context.theme.space(self.padding);
        let background = self.background.map(|role| context.theme.color(role));
        // Padding insets children, so it relayouts. A background is a local fill
        // and only repaints.
        let mut invalidation = Invalidation::empty();
        if state.padding != padding {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.background != background {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            context.ui.update(state.handle, invalidation, |stack| {
                stack.padding = padding;
                stack.background = background;
            })?;
            state.padding = padding;
            state.background = background;
        }
        let mut child_context = context.reborrow(state.handle.id());
        state.children.reconcile(self.children, &mut child_context)
    }
}

struct FlexState<Action: 'static> {
    handle: NodeHandle<Flex>,
    axis: Axis,
    gap: f32,
    padding: f32,
    background: Option<Color>,
    children: MountedChildren<Action>,
}

container_mounted_state!(FlexState);

impl<Action: 'static> DynView<Action> for FlexView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let gap = context.theme.space(self.gap);
        let padding = context.theme.space(self.padding);
        let background = self.background.map(|role| context.theme.color(role));
        let handle = context.ui.append(
            context.parent,
            Flex {
                axis: self.axis,
                gap,
                padding,
                background,
            },
        )?;
        let mut child_context = context.reborrow(handle.id());
        let mut children = MountedChildren::new();
        children.build(self.children, &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(FlexState {
                handle,
                axis: self.axis,
                gap,
                padding,
                background,
                children,
            }),
            marker: std::marker::PhantomData,
        })
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut MountedView<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<FlexState<Action>>()
            .expect("view kind and state agree");
        let gap = context.theme.space(self.gap);
        let padding = context.theme.space(self.padding);
        let background = self.background.map(|role| context.theme.color(role));
        // Axis, gap, and padding all place children; a background is only a
        // local fill behind them.
        let mut invalidation = Invalidation::empty();
        if state.axis != self.axis || state.gap != gap || state.padding != padding {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.background != background {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            context.ui.update(state.handle, invalidation, |flex| {
                flex.axis = self.axis;
                flex.gap = gap;
                flex.padding = padding;
                flex.background = background;
            })?;
            state.axis = self.axis;
            state.gap = gap;
            state.padding = padding;
            state.background = background;
        }
        let mut child_context = context.reborrow(state.handle.id());
        state.children.reconcile(self.children, &mut child_context)
    }
}

/// Strategy chosen for one child-list reconciliation pass.
enum ChildStrategy {
    /// Pair new views with retained ones by position.
    ///
    /// Used for unkeyed lists, and for keyed lists whose length and key order
    /// are both unchanged — which is the overwhelmingly common case, and the one
    /// that used to build and throw away a `HashMap` per container per frame.
    Positional,
    /// Match retained children by key through the reusable index.
    Remap,
}

/// One container's reconciled child list plus the scratch space it reuses.
///
/// Everything here exists to keep a steady-state frame allocation-free: the
/// mounted list is edited in place rather than rebuilt, the key index and the
/// remap buffer are cleared instead of dropped, and the published child order is
/// remembered so an unchanged order is never handed to the engine again.
struct MountedChildren<Action: 'static> {
    mounted: Vec<MountedView<Action>>,
    /// Child order most recently published to the engine.
    published: Vec<NodeId>,
    /// Previous children held during a keyed remap.
    scratch: Vec<Option<MountedView<Action>>>,
    /// Key index reused by validation and by the keyed remap.
    index: HashMap<ViewKey, usize>,
}

impl<Action: 'static> MountedChildren<Action> {
    fn new() -> Self {
        Self {
            mounted: Vec::new(),
            published: Vec::new(),
            scratch: Vec::new(),
            index: HashMap::new(),
        }
    }

    fn mounted_mut(&mut self) -> impl Iterator<Item = &mut MountedView<Action>> {
        self.mounted.iter_mut()
    }

    /// Mounts an initial child list.
    ///
    /// The engine appends children in order, so the published order is recorded
    /// rather than set: there is nothing to reorder yet.
    fn build(
        &mut self,
        views: Vec<AnyView<Action>>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        self.validate(&views)?;
        self.mounted.reserve(views.len());
        self.published.reserve(views.len());
        for view in views {
            let built = view.inner.build_counted(view.key, context)?;
            self.published.push(built.node);
            self.mounted.push(built);
        }
        Ok(())
    }

    /// Reconciles a replacement child list into retained state.
    fn reconcile(
        &mut self,
        views: Vec<AnyView<Action>>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        // `containers_reconciled`: one child-list reconciliation pass. Counted
        // once for both strategies, so every container is covered exactly once.
        ViewStats::record_container_reconciled();
        match self.validate(&views)? {
            ChildStrategy::Positional => self.reconcile_positional(views, context)?,
            ChildStrategy::Remap => self.reconcile_keyed(views, context)?,
        }
        self.publish(context)
    }

    /// Checks keying and picks a strategy in a single scan.
    ///
    /// One pass rejects partially keyed sequences, rejects duplicate keys, and
    /// decides whether the new keys already line up with the retained ones. The
    /// duplicate check reuses `index` instead of allocating a `HashSet` per
    /// container per frame.
    fn validate(&mut self, views: &[AnyView<Action>]) -> Result<ChildStrategy, UiError> {
        let keyed = views.first().is_some_and(|view| view.key.is_some());
        if !keyed {
            if views.iter().any(|view| view.key.is_some()) {
                return Err(UiError::new(
                    "dynamic view sequences must key every child or no children",
                ));
            }
            // An unkeyed list is always paired by position, including when its
            // length changed: trailing children are appended or removed.
            return Ok(ChildStrategy::Positional);
        }
        self.index.clear();
        self.index.reserve(views.len());
        let mut aligned = self.mounted.len() == views.len();
        for (position, view) in views.iter().enumerate() {
            let Some(key) = view.key.as_ref() else {
                return Err(UiError::new(
                    "dynamic view sequences must key every child or no children",
                ));
            };
            if self.index.insert(key.clone(), position).is_some() {
                return Err(UiError::new(format!("duplicate view key `{key}`")));
            }
            if aligned && self.mounted[position].key.as_ref() != Some(key) {
                aligned = false;
            }
        }
        Ok(if aligned {
            ChildStrategy::Positional
        } else {
            ChildStrategy::Remap
        })
    }

    fn reconcile_positional(
        &mut self,
        views: Vec<AnyView<Action>>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let kept = views.len();
        for (position, view) in views.into_iter().enumerate() {
            if let Some(retained) = self.mounted.get_mut(position) {
                if retained.kind == view.kind() {
                    view.inner.rebuild_counted(retained, context)?;
                } else {
                    context.ui.remove(retained.node)?;
                    *retained = view.inner.build_counted(view.key, context)?;
                }
            } else {
                let built = view.inner.build_counted(view.key, context)?;
                self.mounted.push(built);
            }
        }
        while self.mounted.len() > kept {
            let extra = self.mounted.pop().expect("length was checked");
            context.ui.remove(extra.node)?;
        }
        Ok(())
    }

    fn reconcile_keyed(
        &mut self,
        views: Vec<AnyView<Action>>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        // Re-purpose the index built by `validate` as retained key -> slot.
        self.index.clear();
        self.scratch.clear();
        for retained in self.mounted.drain(..) {
            let key = retained.key.clone().expect("validated keyed sequence");
            self.index.insert(key, self.scratch.len());
            self.scratch.push(Some(retained));
        }
        for view in views {
            let key = view.key.clone().expect("validated keyed sequence");
            let retained = self
                .index
                .get(&key)
                .copied()
                .and_then(|slot| self.scratch[slot].take());
            match retained {
                Some(mut retained) if retained.kind == view.kind() => {
                    view.inner.rebuild_counted(&mut retained, context)?;
                    self.mounted.push(retained);
                }
                Some(retained) => {
                    context.ui.remove(retained.node)?;
                    self.mounted
                        .push(view.inner.build_counted(Some(key), context)?);
                }
                None => self
                    .mounted
                    .push(view.inner.build_counted(Some(key), context)?),
            }
        }
        for slot in 0..self.scratch.len() {
            if let Some(extra) = self.scratch[slot].take() {
                context.ui.remove(extra.node)?;
            }
        }
        self.scratch.clear();
        Ok(())
    }

    /// Publishes the child order, but only when it actually moved.
    ///
    /// `UiRoot::set_children` invalidates the parent with `Invalidation::ALL`,
    /// so republishing an unchanged order costs a full pass over a container
    /// that did not change at all. Comparing against the last published order
    /// makes that cost proportional to real structural change.
    fn publish(&mut self, context: &mut ViewContext<'_, Action>) -> Result<(), UiError> {
        if self.published.len() == self.mounted.len()
            && self
                .published
                .iter()
                .zip(&self.mounted)
                .all(|(node, view)| *node == view.node)
        {
            return Ok(());
        }
        self.published.clear();
        self.published
            .extend(self.mounted.iter().map(|view| view.node));
        // `set_children_calls`: the only place the framework hands the engine a
        // whole child list.
        ViewStats::record_set_children();
        context.ui.set_children(context.parent, &self.published)
    }
}
