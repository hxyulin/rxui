//! Lightweight view values and keyed reconciliation.

use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    fmt,
    ops::RangeInclusive,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use astrelis_core::{color::Color, geometry::LogicalSize};
use astrelis_ui_next::{
    Axis, BoxElement, Button, Checkbox, Flex, Invalidation, Label, NodeHandle, NodeId,
    SemanticData, Slider, TextField, UiError, UiRoot,
};

use crate::{
    ButtonVariant, ColorRole, ComponentContext, ComponentWithProps, RoutedComponentAction, Space,
    Theme,
};

static NEXT_COMPONENT_ID: AtomicU64 = AtomicU64::new(1);

/// Stable view identity used by dynamic collections.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViewKey(Arc<str>);

impl ViewKey {
    /// Creates a key.
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }
}

impl From<&str> for ViewKey {
    fn from(value: &str) -> Self {
        Self::new(Arc::<str>::from(value))
    }
}

impl From<String> for ViewKey {
    fn from(value: String) -> Self {
        Self::new(Arc::<str>::from(value))
    }
}

impl From<u64> for ViewKey {
    fn from(value: u64) -> Self {
        Self::new(Arc::<str>::from(value.to_string()))
    }
}

impl fmt::Display for ViewKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
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
    pub(crate) ui: &'a mut UiRoot,
    pub(crate) theme: &'a Theme,
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

struct ViewContext<'a, Action: 'static> {
    ui: &'a mut UiRoot,
    parent: NodeId,
    theme: &'a Theme,
    action_sink: Arc<dyn Fn(Action) -> Box<dyn Any>>,
    marker: std::marker::PhantomData<fn() -> Action>,
}

impl<Action: 'static> ViewContext<'_, Action> {
    fn reborrow(&mut self, parent: NodeId) -> ViewContext<'_, Action> {
        ViewContext {
            ui: self.ui,
            parent,
            theme: self.theme,
            action_sink: self.action_sink.clone(),
            marker: std::marker::PhantomData,
        }
    }
}

/// Mounts and incrementally reconciles one root view.
pub struct ViewHost<Action: 'static> {
    mounted: MountedView<Action>,
}

impl<Action: 'static> ViewHost<Action> {
    /// Builds the initial retained subtree.
    pub fn mount(ui: &mut UiRoot, theme: &Theme, view: AnyView<Action>) -> Result<Self, UiError> {
        let mut context = ViewContext {
            parent: ui.root(),
            ui,
            theme,
            action_sink: Arc::new(|action| Box::new(action)),
            marker: std::marker::PhantomData,
        };
        let mounted = view.inner.build(view.key, &mut context)?;
        Ok(Self { mounted })
    }

    /// Reconciles a replacement root view into retained state.
    pub fn rebuild(
        &mut self,
        ui: &mut UiRoot,
        theme: &Theme,
        view: AnyView<Action>,
    ) -> Result<(), UiError> {
        let mut context = ViewContext {
            parent: ui.root(),
            ui,
            theme,
            action_sink: Arc::new(|action| Box::new(action)),
            marker: std::marker::PhantomData,
        };
        if self.mounted.kind == view.kind() {
            view.inner.rebuild(&mut self.mounted, &mut context)?;
        } else {
            context.ui.remove(self.mounted.node)?;
            self.mounted = view.inner.build(view.key, &mut context)?;
        }
        Ok(())
    }

    /// Root retained identity.
    pub const fn node(&self) -> NodeId {
        self.mounted.node
    }

    pub(crate) fn route(
        &mut self,
        ui: &mut UiRoot,
        theme: &Theme,
        action: crate::RoutedComponentAction,
    ) -> Result<Vec<Action>, UiError> {
        let mut context = RouteContext { ui, theme };
        let output = self.mounted.state.route(&mut Some(action), &mut context)?;
        Ok(output)
    }
}

/// Creates a text view.
pub fn label<Action: 'static>(text: impl Into<String>) -> AnyView<Action> {
    label_with_width(text, None)
}

/// Creates a shaped text view with an optional preferred width.
pub fn label_with_width<Action: 'static>(
    text: impl Into<String>,
    width: impl Into<Option<f32>>,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(LabelView {
            text: text.into(),
            font_size: 14.0,
            role: ColorRole::Text,
            width: width.into(),
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
            size: LogicalSize::new(120.0, 30.0),
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

struct ComponentView<C: ComponentWithProps, Parent: 'static> {
    props: C::Props,
    map_effect: Arc<dyn Fn(C::Effect) -> Parent>,
}

struct ComponentState<C: ComponentWithProps, Parent: 'static> {
    id: u64,
    parent: NodeHandle<Flex>,
    props: C::Props,
    component: C,
    child: MountedView<C::Action>,
    map_effect: Arc<dyn Fn(C::Effect) -> Parent>,
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

    fn rebuild_child(&mut self, context: &mut RouteContext<'_>) -> Result<(), UiError> {
        let view = self.component.view(context.theme);
        let mut child_context = ViewContext {
            ui: context.ui,
            parent: self.parent.id(),
            theme: context.theme,
            action_sink: Self::action_sink(self.id),
            marker: std::marker::PhantomData,
        };
        if self.child.kind == view.kind() {
            view.inner.rebuild(&mut self.child, &mut child_context)
        } else {
            child_context.ui.remove(self.child.node)?;
            self.child = view.inner.build(view.key, &mut child_context)?;
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
            self.component
                .update(action, &mut ComponentContext::new(&mut effects));
        }
        self.rebuild_child(context)?;
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
}

impl<C: ComponentWithProps, Parent: 'static> DynView<Parent> for ComponentView<C, Parent> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Parent>,
    ) -> Result<MountedView<Parent>, UiError> {
        let id = NEXT_COMPONENT_ID.fetch_add(1, Ordering::Relaxed);
        let parent = context.ui.append(context.parent, Flex::default())?;
        let component = C::create(&self.props);
        let view = component.view(context.theme);
        let mut child_context = ViewContext {
            ui: context.ui,
            parent: parent.id(),
            theme: context.theme,
            action_sink: ComponentState::<C, Parent>::action_sink(id),
            marker: std::marker::PhantomData,
        };
        let child = view.inner.build(view.key, &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: parent.id(),
            state: Box::new(ComponentState::<C, Parent> {
                id,
                parent,
                props: self.props,
                component,
                child,
                map_effect: self.map_effect,
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
        state.map_effect = self.map_effect;
        if state.props != self.props {
            state.component.changed(&self.props);
            state.props = self.props;
        }
        state.rebuild_child(&mut RouteContext {
            ui: context.ui,
            theme: context.theme,
        })
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

struct EnabledState<Action: 'static> {
    child: MountedView<Action>,
    enabled: bool,
}

impl<Action: 'static> MountedState<Action> for EnabledState<Action> {
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
}

impl<Action: 'static> DynView<Action> for EnabledView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let child = self.child.inner.build(self.child.key.clone(), context)?;
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
            self.child.inner.rebuild(&mut state.child, context)?;
        } else {
            context.ui.remove(state.child.node)?;
            state.child = self.child.inner.build(self.child.key.clone(), context)?;
            mounted.node = state.child.node;
        }
        if state.enabled != self.enabled {
            context.ui.set_enabled(state.child.node, self.enabled)?;
        }
        state.enabled = self.enabled;
        Ok(())
    }
}

struct MapActionState<Child: 'static, Parent: 'static> {
    child: MountedView<Child>,
    map: Arc<dyn Fn(Child) -> Parent>,
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
        self.child.state.route(action, context).map(|actions| {
            actions
                .into_iter()
                .map(|action| (self.map)(action))
                .collect()
        })
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
        let parent_sink = context.action_sink.clone();
        let map = self.map.clone();
        let mut child_context = ViewContext {
            ui: context.ui,
            parent: context.parent,
            theme: context.theme,
            action_sink: Arc::new(move |action| parent_sink(map(action))),
            marker: std::marker::PhantomData,
        };
        let child = self
            .child
            .inner
            .build(self.child.key.clone(), &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: child.node,
            state: Box::new(MapActionState {
                child,
                map: self.map,
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
        let parent_sink = context.action_sink.clone();
        let map = self.map.clone();
        state.map = self.map.clone();
        let mut child_context = ViewContext {
            ui: context.ui,
            parent: context.parent,
            theme: context.theme,
            action_sink: Arc::new(move |action| parent_sink(map(action))),
            marker: std::marker::PhantomData,
        };
        if state.child.kind == self.child.kind() {
            self.child
                .inner
                .rebuild(&mut state.child, &mut child_context)?;
        } else {
            child_context.ui.remove(state.child.node)?;
            state.child = self
                .child
                .inner
                .build(self.child.key.clone(), &mut child_context)?;
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

struct LabelView {
    text: String,
    font_size: f32,
    role: ColorRole,
    width: Option<f32>,
}

struct LabelState {
    handle: NodeHandle<Label>,
    text: String,
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
        let mut label = Label::new(self.text.clone())
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
        if state.text != self.text
            || state.font_size != self.font_size
            || state.width != self.width
            || state.color != color
        {
            context.ui.edit(state.handle).set_content(
                self.text.clone(),
                self.font_size,
                color,
                self.width,
            )?;
        }
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
        if !invalidation.is_empty() {
            context.ui.edit(state.handle).set_box(
                self.size,
                color,
                self.semantics.clone(),
                self.interactive,
            )?;
        }
        state.size = self.size;
        state.color = color;
        state.semantics = self.semantics;
        Ok(())
    }
}

struct ButtonView<Action> {
    text: String,
    action: Action,
    variant: ButtonVariant,
    size: LogicalSize,
}

struct ButtonState {
    handle: NodeHandle<Button>,
    text: String,
    variant: ButtonVariant,
    size: LogicalSize,
    colors: (Color, Color),
}

leaf_mounted_state!(ButtonState);

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
        let action = self.action.clone();
        let action_sink = context.action_sink.clone();
        let handle = context.ui.append(
            context.parent,
            Button::with_action_factory(
                self.text.clone(),
                self.size,
                colors.0,
                colors.1,
                move || action_sink(action.clone()),
            ),
        )?;
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
            .downcast_mut::<ButtonState>()
            .expect("view kind and state agree");
        let colors = context.theme.button(self.variant);
        let action = self.action.clone();
        let action_sink = context.action_sink.clone();
        let invalidation = if state.size != self.size || state.text != self.text {
            Invalidation::LAYOUT_ALL
        } else if state.colors != colors {
            Invalidation::PAINT
        } else {
            Invalidation::empty()
        };
        if !invalidation.is_empty() {
            context.ui.edit(state.handle).set_button(
                self.text.clone(),
                self.size,
                colors.0,
                colors.1,
            )?;
        }
        context
            .ui
            .edit(state.handle)
            .set_action(move || action_sink(action.clone()))?;
        state.text = self.text;
        state.variant = self.variant;
        state.size = self.size;
        state.colors = colors;
        Ok(())
    }
}

struct TextFieldView<Action: 'static> {
    label: String,
    value: String,
    on_changed: Arc<dyn Fn(String) -> Action>,
}

struct TextFieldState {
    handle: NodeHandle<TextField>,
    label: String,
    value: String,
    colors: (Color, Color),
}

leaf_mounted_state!(TextFieldState);

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
        let on_changed = self.on_changed.clone();
        let action_sink = context.action_sink.clone();
        let mut field = TextField::new(self.label.clone(), self.value.clone())
            .on_changed_factory(move |value| action_sink(on_changed(value)));
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
            .downcast_mut::<TextFieldState>()
            .expect("view kind and state agree");
        let colors = (
            context.theme.color(ColorRole::Text),
            context.theme.color(ColorRole::Surface),
        );
        let invalidation = if state.value != self.value || state.label != self.label {
            Invalidation::LAYOUT_ALL
        } else if state.colors != colors {
            Invalidation::PAINT
        } else {
            Invalidation::empty()
        };
        let on_changed = self.on_changed.clone();
        let action_sink = context.action_sink.clone();
        let mut field = context.ui.edit(state.handle);
        if !invalidation.is_empty() {
            let update_changed = on_changed.clone();
            let update_sink = action_sink.clone();
            field.set_field(
                self.label.clone(),
                self.value.clone(),
                colors.0,
                colors.1,
                move |value| update_sink(update_changed(value)),
            )?;
        }
        field.set_change_action(move |value| action_sink(on_changed(value)))?;
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

struct CheckboxState {
    handle: NodeHandle<Checkbox>,
    label: String,
    checked: bool,
    colors: (Color, Color, Color),
}

leaf_mounted_state!(CheckboxState);

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
        let on_changed = self.on_changed.clone();
        let action_sink = context.action_sink.clone();
        let mut checkbox = Checkbox::new(self.label.clone(), self.checked, move |checked| {
            action_sink(on_changed(checked))
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
            .downcast_mut::<CheckboxState>()
            .expect("view kind and state agree");
        let colors = (
            context.theme.color(ColorRole::Text),
            context.theme.color(ColorRole::Muted),
            context.theme.color(ColorRole::Accent),
        );
        let mut checkbox = context.ui.edit(state.handle);
        if state.label != self.label || state.checked != self.checked || state.colors != colors {
            checkbox.set_checkbox(
                self.label.clone(),
                self.checked,
                colors.0,
                colors.1,
                colors.2,
            )?;
        }
        let on_changed = self.on_changed.clone();
        let action_sink = context.action_sink.clone();
        checkbox.set_change_action(move |checked| action_sink(on_changed(checked)))?;
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

struct SliderState {
    handle: NodeHandle<Slider>,
    label: String,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    colors: (Color, Color),
}

leaf_mounted_state!(SliderState);

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
        let on_changed = self.on_changed.clone();
        let action_sink = context.action_sink.clone();
        let mut slider = Slider::new(
            self.label.clone(),
            self.value,
            self.range.clone(),
            move |value| action_sink(on_changed(value)),
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
            .downcast_mut::<SliderState>()
            .expect("view kind and state agree");
        let colors = (
            context.theme.color(ColorRole::Muted),
            context.theme.color(ColorRole::Accent),
        );
        let mut slider = context.ui.edit(state.handle);
        if state.label != self.label
            || state.value != self.value
            || state.range != self.range
            || state.step != self.step
            || state.colors != colors
        {
            slider.set_slider(
                self.label.clone(),
                self.value,
                self.range.clone(),
                self.step,
                colors.0,
                colors.1,
            )?;
        }
        let on_changed = self.on_changed.clone();
        let action_sink = context.action_sink.clone();
        slider.set_change_action(move |value| action_sink(on_changed(value)))?;
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

struct FlexState<Action: 'static> {
    handle: NodeHandle<Flex>,
    axis: Axis,
    gap: f32,
    padding: f32,
    background: Option<Color>,
    children: Vec<MountedView<Action>>,
}

impl<Action: 'static> MountedState<Action> for FlexState<Action> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn route(
        &mut self,
        action: &mut Option<crate::RoutedComponentAction>,
        context: &mut RouteContext<'_>,
    ) -> Result<Vec<Action>, UiError> {
        let mut output = Vec::new();
        for child in &mut self.children {
            output.extend(child.state.route(action, context)?);
            if action.is_none() {
                break;
            }
        }
        Ok(output)
    }
}

impl<Action: 'static> DynView<Action> for FlexView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        validate_keys(&self.children)?;
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
        let mut children = Vec::with_capacity(self.children.len());
        for child in self.children {
            children.push(child.inner.build(child.key, &mut child_context)?);
        }
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
        validate_keys(&self.children)?;
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<FlexState<Action>>()
            .expect("view kind and state agree");
        let gap = context.theme.space(self.gap);
        let padding = context.theme.space(self.padding);
        let background = self.background.map(|role| context.theme.color(role));
        if state.axis != self.axis
            || state.gap != gap
            || state.padding != padding
            || state.background != background
        {
            context
                .ui
                .edit(state.handle)
                .set_flex(self.axis, gap, padding, background)?;
        }
        let mut child_context = context.reborrow(state.handle.id());
        reconcile_children(&mut state.children, self.children, &mut child_context)?;
        state.axis = self.axis;
        state.gap = gap;
        state.padding = padding;
        state.background = background;
        Ok(())
    }
}

fn validate_keys<Action: 'static>(views: &[AnyView<Action>]) -> Result<(), UiError> {
    let keyed = views.iter().filter(|view| view.key.is_some()).count();
    if keyed != 0 && keyed != views.len() {
        return Err(UiError::new(
            "dynamic view sequences must key every child or no children",
        ));
    }
    let mut keys = HashSet::new();
    for key in views.iter().filter_map(|view| view.key.as_ref()) {
        if !keys.insert(key) {
            return Err(UiError::new(format!("duplicate view key `{key}`")));
        }
    }
    Ok(())
}

fn reconcile_children<Action: 'static>(
    mounted: &mut Vec<MountedView<Action>>,
    views: Vec<AnyView<Action>>,
    context: &mut ViewContext<'_, Action>,
) -> Result<(), UiError> {
    let keyed = views.first().is_some_and(|view| view.key.is_some());
    let old = std::mem::take(mounted);
    let mut next = Vec::with_capacity(views.len());
    if keyed {
        let mut old = old
            .into_iter()
            .map(|mounted| {
                (
                    mounted.key.clone().expect("validated keyed sequence"),
                    mounted,
                )
            })
            .collect::<HashMap<_, _>>();
        for view in views {
            let key = view.key.clone().expect("validated keyed sequence");
            if let Some(mut retained) = old.remove(&key) {
                if retained.kind == view.kind() {
                    view.inner.rebuild(&mut retained, context)?;
                    next.push(retained);
                } else {
                    context.ui.remove(retained.node)?;
                    next.push(view.inner.build(Some(key), context)?);
                }
            } else {
                next.push(view.inner.build(Some(key), context)?);
            }
        }
        for retained in old.into_values() {
            context.ui.remove(retained.node)?;
        }
    } else {
        let mut old = old.into_iter();
        for view in views {
            if let Some(mut retained) = old.next() {
                if retained.kind == view.kind() {
                    view.inner.rebuild(&mut retained, context)?;
                    next.push(retained);
                } else {
                    context.ui.remove(retained.node)?;
                    next.push(view.inner.build(None, context)?);
                }
            } else {
                next.push(view.inner.build(None, context)?);
            }
        }
        for retained in old {
            context.ui.remove(retained.node)?;
        }
    }
    context.ui.set_children(
        context.parent,
        &next.iter().map(|view| view.node).collect::<Vec<_>>(),
    )?;
    *mounted = next;
    Ok(())
}
