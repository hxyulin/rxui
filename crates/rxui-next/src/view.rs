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

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize},
};
use astrelis_ui_next::{
    Align, Alignment, Axis, BoxElement, Button, Checkbox, Element, Flex, Frame, Invalidation,
    KeyListener, Label, NodeHandle, NodeId, Scroll, ScrollAxis, SemanticData, Slider, SplitPane,
    Stack, TextField, UiError, UiRoot,
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
    label_with_style(text, LabelStyle::default())
}

/// Creates a shaped text view with an optional preferred width.
pub fn label_with_width<Action: 'static>(
    text: impl Into<String>,
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
    text: impl Into<String>,
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

impl<Action: 'static> MountedState<Action> for FocusScopeState<Action> {
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
        let child = self.child.inner.build(self.child.key.clone(), context)?;
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
            self.child.inner.rebuild(&mut state.child, context)?;
        } else {
            context.ui.remove(state.child.node)?;
            state.child = self.child.inner.build(self.child.key.clone(), context)?;
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
}

impl<Action: Clone + 'static> MountedState<Action> for EscapeState<Action> {
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

impl<Action: Clone + 'static> DynView<Action> for EscapeView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let action = self.action.clone();
        let action_sink = context.action_sink.clone();
        let handle = context.ui.append(
            context.parent,
            KeyListener::on_escape(move || action_sink(action.clone())),
        )?;
        let mut child_context = context.reborrow(handle.id());
        let child = self
            .child
            .inner
            .build(self.child.key.clone(), &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(EscapeState { handle, child }),
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
        let action = self.action;
        let action_sink = context.action_sink.clone();
        context
            .ui
            .update(state.handle, Invalidation::empty(), |listener| {
                listener.set_escape(move || action_sink(action.clone()));
            })?;
        let mut child_context = context.reborrow(state.handle.id());
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
}

impl<Action: Clone + 'static> MountedState<Action> for CommandNavigationState<Action> {
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

impl<Action: Clone + 'static> DynView<Action> for CommandNavigationView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let previous = self.previous.clone();
        let next = self.next.clone();
        let submit = self.submit.clone();
        let previous_sink = context.action_sink.clone();
        let next_sink = context.action_sink.clone();
        let submit_sink = context.action_sink.clone();
        let handle = context.ui.append(
            context.parent,
            KeyListener::command_navigation(
                move || previous_sink(previous.clone()),
                move || next_sink(next.clone()),
                move || submit_sink(submit.clone()),
            ),
        )?;
        let mut child_context = context.reborrow(handle.id());
        let child = self
            .child
            .inner
            .build(self.child.key.clone(), &mut child_context)?;
        Ok(MountedView {
            key,
            kind: TypeId::of::<Self>(),
            node: handle.id(),
            state: Box::new(CommandNavigationState { handle, child }),
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
        let previous = self.previous;
        let next = self.next;
        let submit = self.submit;
        let previous_sink = context.action_sink.clone();
        let next_sink = context.action_sink.clone();
        let submit_sink = context.action_sink.clone();
        context
            .ui
            .update(state.handle, Invalidation::empty(), |listener| {
                listener.set_command_navigation(
                    move || previous_sink(previous.clone()),
                    move || next_sink(next.clone()),
                    move || submit_sink(submit.clone()),
                );
            })?;
        let mut child_context = context.reborrow(state.handle.id());
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

impl<Action: 'static> MountedState<Action> for AlignState<Action> {
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
            .build(self.child.key.clone(), &mut child_context)?;
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
                .rebuild(&mut state.child, &mut child_context)?;
        } else {
            child_context.ui.remove(state.child.node)?;
            state.child = self
                .child
                .inner
                .build(self.child.key.clone(), &mut child_context)?;
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

impl<Action: 'static> MountedState<Action> for FrameState<Action> {
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
            .build(self.child.key.clone(), &mut child_context)?;
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
            context.ui.edit(state.handle).set_frame(
                self.style.width,
                self.style.height,
                self.style.min,
                self.style.max,
                self.style.grow,
            )?;
        }
        let mut child_context = context.reborrow(state.handle.id());
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

impl<Action: 'static> MountedState<Action> for VisibleState<Action> {
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

impl<Action: 'static> DynView<Action> for VisibleView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        let child = self.child.inner.build(self.child.key.clone(), context)?;
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
            self.child.inner.rebuild(&mut state.child, context)?;
        } else {
            context.ui.remove(state.child.node)?;
            state.child = self.child.inner.build(self.child.key.clone(), context)?;
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
    children: Vec<MountedView<Action>>,
}

impl<Action: 'static> MountedState<Action> for SplitPaneState<Action> {
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

impl<Action: 'static> DynView<Action> for SplitPaneView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        validate_keys(&self.children)?;
        let on_resize = self.on_resize.clone();
        let action_sink = context.action_sink.clone();
        let handle = context.ui.append(
            context.parent,
            SplitPane::new(self.axis, self.ratio, move |ratio| {
                action_sink(on_resize(ratio))
            }),
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
            state: Box::new(SplitPaneState {
                handle,
                axis: self.axis,
                ratio: self.ratio,
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
            .downcast_mut::<SplitPaneState<Action>>()
            .expect("view kind and state agree");
        let on_resize = self.on_resize.clone();
        let action_sink = context.action_sink.clone();
        context
            .ui
            .update(state.handle, Invalidation::LAYOUT_ALL, |split| {
                split.axis = self.axis;
                split.ratio = self.ratio;
                split.set_changed(move |ratio| action_sink(on_resize(ratio)));
            })?;
        let mut child_context = context.reborrow(state.handle.id());
        reconcile_children(&mut state.children, self.children, &mut child_context)?;
        state.axis = self.axis;
        state.ratio = self.ratio;
        Ok(())
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
    children: Vec<MountedView<Action>>,
}

impl<Action: 'static> MountedState<Action> for ScrollState<Action> {
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

impl<Action: 'static> DynView<Action> for ScrollView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        validate_keys(&self.children)?;
        let handle = context.ui.append(context.parent, Scroll::new(self.axis))?;
        context.ui.edit(handle).set_scroll(self.axis, self.offset)?;
        let mut child_context = context.reborrow(handle.id());
        let mut children = Vec::with_capacity(self.children.len());
        for child in self.children {
            children.push(child.inner.build(child.key, &mut child_context)?);
        }
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
        validate_keys(&self.children)?;
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<ScrollState<Action>>()
            .expect("view kind and state agree");
        if state.axis != self.axis || state.requested_offset != self.offset {
            context
                .ui
                .edit(state.handle)
                .set_scroll(self.axis, self.offset)?;
        }
        let mut child_context = context.reborrow(state.handle.id());
        reconcile_children(&mut state.children, self.children, &mut child_context)?;
        state.axis = self.axis;
        state.requested_offset = self.offset;
        Ok(())
    }
}

struct StackState<Action: 'static> {
    handle: NodeHandle<Stack>,
    padding: f32,
    background: Option<Color>,
    children: Vec<MountedView<Action>>,
}

impl<Action: 'static> MountedState<Action> for StackState<Action> {
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

impl<Action: 'static> DynView<Action> for StackView<Action> {
    fn kind(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn build(
        self: Box<Self>,
        key: Option<ViewKey>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<MountedView<Action>, UiError> {
        validate_keys(&self.children)?;
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
        let mut children = Vec::with_capacity(self.children.len());
        for child in self.children {
            children.push(child.inner.build(child.key, &mut child_context)?);
        }
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
        validate_keys(&self.children)?;
        let state = mounted
            .state
            .as_any_mut()
            .downcast_mut::<StackState<Action>>()
            .expect("view kind and state agree");
        let padding = context.theme.space(self.padding);
        let background = self.background.map(|role| context.theme.color(role));
        if state.padding != padding || state.background != background {
            context
                .ui
                .edit(state.handle)
                .set_stack(padding, background)?;
        }
        let mut child_context = context.reborrow(state.handle.id());
        reconcile_children(&mut state.children, self.children, &mut child_context)?;
        state.padding = padding;
        state.background = background;
        Ok(())
    }
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
