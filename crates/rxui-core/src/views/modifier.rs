//! Wrappers that adjust one child without owning a child list.

use std::{any::Any, cell::RefCell, rc::Rc, sync::Arc};

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::{
    Align, Alignment, Frame, Invalidation, KeyListener, NodeHandle, NodeId, UiError,
};

use crate::{
    FrameStyle, RoutedComponentAction, Space,
    view::{
        ActionCell, ActionEmitter, AnyView, Mounted, MountedState, RebuildContext, RouteContext,
        ViewContext, ViewNode, wrapper_mounted_state,
    },
};
impl<Action: 'static> AnyView<Action> {
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
    child: Mounted<Action>,
    active: bool,
    previous: Option<NodeId>,
}

wrapper_mounted_state!(FocusScopeState);

impl<Action: 'static> ViewNode<Action> for FocusScopeView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let previous = self
            .active
            .then(|| {
                context
                    .ui()
                    .focused()
                    .or_else(|| context.ui().last_focused())
            })
            .flatten();
        let child = context.build_child(self.child)?;
        if self.active {
            context.ui().focus_first_in_subtree(child.node())?;
        }
        let node = child.node();
        Ok(Mounted::new(
            node,
            FocusScopeState {
                child,
                active: self.active,
                previous,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted.state_mut::<FocusScopeState<Action>>()?;
        let activating = !state.active && self.active;
        let deactivating = state.active && !self.active;
        let previous = activating
            .then(|| {
                context
                    .ui()
                    .focused()
                    .or_else(|| context.ui().last_focused())
            })
            .flatten();
        context.rebuild_child(&mut state.child, self.child)?;
        let child_node = state.child.node();
        if activating {
            state.previous = previous;
            context.ui().focus_first_in_subtree(child_node)?;
        } else if deactivating {
            let restored = state
                .previous
                .filter(|previous| context.ui().contains(*previous))
                .is_some_and(|previous| context.ui().set_focus(Some(previous)).is_ok());
            if !restored {
                context.ui().set_focus(None)?;
            }
            state.previous = None;
        }
        state.active = self.active;
        mounted.set_node(child_node);
        Ok(())
    }
}

struct EscapeView<Action: Clone + 'static> {
    child: AnyView<Action>,
    action: Action,
}

struct EscapeState<Action: Clone + 'static> {
    handle: NodeHandle<KeyListener>,
    child: Mounted<Action>,
    action: ActionCell<Action>,
}

wrapper_mounted_state!(EscapeState where Action: Clone);

impl<Action: Clone + 'static> ViewNode<Action> for EscapeView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let action = ActionCell::new(self.action, context.emitter());
        let emit = action.emitter();
        let handle = context.append(KeyListener::on_escape(move || emit()))?;
        let child = context.child(handle.id()).build_child(self.child)?;
        Ok(Mounted::new(
            handle.id(),
            EscapeState {
                handle,
                child,
                action,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let emitter = context.emitter();
        let state = mounted.state_mut::<EscapeState<Action>>()?;
        // Writing the cell replaces the dismissal action without reinstalling a
        // listener closure, which is why no retained update happens here at all.
        state.action.update(self.action, &emitter);
        let parent = state.handle.id();
        context
            .child(parent)
            .rebuild_child(&mut state.child, self.child)
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
    child: Mounted<Action>,
    previous: ActionCell<Action>,
    next: ActionCell<Action>,
    submit: ActionCell<Action>,
}

wrapper_mounted_state!(CommandNavigationState where Action: Clone);

impl<Action: Clone + 'static> ViewNode<Action> for CommandNavigationView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let previous = ActionCell::new(self.previous, context.emitter());
        let next = ActionCell::new(self.next, context.emitter());
        let submit = ActionCell::new(self.submit, context.emitter());
        let emit_previous = previous.emitter();
        let emit_next = next.emitter();
        let emit_submit = submit.emitter();
        let handle = context.append(KeyListener::command_navigation(
            move || emit_previous(),
            move || emit_next(),
            move || emit_submit(),
        ))?;
        let child = context.child(handle.id()).build_child(self.child)?;
        Ok(Mounted::new(
            handle.id(),
            CommandNavigationState {
                handle,
                child,
                previous,
                next,
                submit,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let emitter = context.emitter();
        let state = mounted.state_mut::<CommandNavigationState<Action>>()?;
        state.previous.update(self.previous, &emitter);
        state.next.update(self.next, &emitter);
        state.submit.update(self.submit, &emitter);
        let parent = state.handle.id();
        context
            .child(parent)
            .rebuild_child(&mut state.child, self.child)
    }
}

struct AlignView<Action: 'static> {
    child: AnyView<Action>,
    alignment: Alignment,
    padding: Space,
}

struct AlignState<Action: 'static> {
    handle: NodeHandle<Align>,
    child: Mounted<Action>,
    alignment: Alignment,
    padding: f32,
}

wrapper_mounted_state!(AlignState);

impl<Action: 'static> ViewNode<Action> for AlignView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let padding = context.theme().space(self.padding);
        let handle = context.append(Align {
            alignment: self.alignment,
            padding,
        })?;
        let child = context.child(handle.id()).build_child(self.child)?;
        Ok(Mounted::new(
            handle.id(),
            AlignState {
                handle,
                child,
                alignment: self.alignment,
                padding,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let padding = context.theme().space(self.padding);
        let state = mounted.state_mut::<AlignState<Action>>()?;
        if state.alignment != self.alignment || state.padding != padding {
            let alignment = self.alignment;
            context
                .ui()
                .update(state.handle, Invalidation::LAYOUT_ALL, |align| {
                    align.alignment = alignment;
                    align.padding = padding;
                })?;
            state.alignment = alignment;
            state.padding = padding;
        }
        let parent = state.handle.id();
        context
            .child(parent)
            .rebuild_child(&mut state.child, self.child)
    }
}

struct FrameState<Action: 'static> {
    handle: NodeHandle<Frame>,
    child: Mounted<Action>,
    style: FrameStyle,
}

wrapper_mounted_state!(FrameState);

impl<Action: 'static> ViewNode<Action> for FrameView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let handle = context.append(Frame {
            width: self.style.width,
            height: self.style.height,
            min: self.style.min,
            max: self.style.max,
            grow: self.style.grow,
        })?;
        let child = context.child(handle.id()).build_child(self.child)?;
        Ok(Mounted::new(
            handle.id(),
            FrameState {
                handle,
                child,
                style: self.style,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted.state_mut::<FrameState<Action>>()?;
        if state.style != self.style {
            // Every field of a frame is a layout input, so `LAYOUT_ALL` is the
            // exact answer. Normalization mirrors what `set_frame` guaranteed.
            let style = self.style;
            context
                .ui()
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
            state.style = style;
        }
        let parent = state.handle.id();
        context
            .child(parent)
            .rebuild_child(&mut state.child, self.child)
    }
}

struct VisibleView<Action: 'static> {
    child: AnyView<Action>,
    visible: bool,
}

struct VisibleState<Action: 'static> {
    child: Mounted<Action>,
    visible: bool,
}

wrapper_mounted_state!(VisibleState);

impl<Action: 'static> ViewNode<Action> for VisibleView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let child = context.build_child(self.child)?;
        let node = child.node();
        context.ui().set_visible(node, self.visible)?;
        Ok(Mounted::new(
            node,
            VisibleState {
                child,
                visible: self.visible,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted.state_mut::<VisibleState<Action>>()?;
        // A replacement, not a reconcile, is the case the declared-value guard
        // alone gets wrong: `rebuild_child` builds a fresh node when the child's
        // kind changes, and a fresh node is visible. Comparing only what this
        // wrapper declared would then see no change and write nothing, so
        // swapping the view under a `.visible(false)` would reveal it.
        let previous = state.child.node();
        context.rebuild_child(&mut state.child, self.child)?;
        let node = state.child.node();
        if state.visible != self.visible || node != previous {
            context.ui().set_visible(node, self.visible)?;
        }
        state.visible = self.visible;
        mounted.set_node(node);
        Ok(())
    }
}

struct EnabledState<Action: 'static> {
    child: Mounted<Action>,
    enabled: bool,
}

wrapper_mounted_state!(EnabledState);

impl<Action: 'static> ViewNode<Action> for EnabledView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let child = context.build_child(self.child)?;
        let node = child.node();
        context.ui().set_enabled(node, self.enabled)?;
        Ok(Mounted::new(
            node,
            EnabledState {
                child,
                enabled: self.enabled,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted.state_mut::<EnabledState<Action>>()?;
        // See `VisibleView::rebuild`: a fresh node arrives enabled, so the
        // declared-value guard has to be paired with an identity check.
        let previous = state.child.node();
        context.rebuild_child(&mut state.child, self.child)?;
        let node = state.child.node();
        if state.enabled != self.enabled || node != previous {
            context.ui().set_enabled(node, self.enabled)?;
        }
        state.enabled = self.enabled;
        mounted.set_node(node);
        Ok(())
    }
}

/// Mutable inputs of a composed action emitter.
///
/// [`AnyView::map_action`] takes a fresh closure every frame, so the mapping's
/// identity can never be compared and a naive rebuild has to allocate a new
/// composed emitter each pass. Every retained element below the wrapper then
/// sees a different sink identity on every frame, which is invisible today but
/// makes any future memo keyed on sink identity miss unconditionally. Routing
/// through this cell keeps the composed emitter allocated exactly once at mount
/// while still calling the newest mapping.
struct MapActionCell<Child: 'static, Parent: 'static> {
    map: Arc<dyn Fn(Child) -> Parent>,
    parent: ActionEmitter<Parent>,
}

impl<Child: 'static, Parent: 'static> MapActionCell<Child, Parent> {
    /// Builds the single composed emitter that reads through `cell` forever.
    fn compose(cell: &Rc<RefCell<Self>>) -> ActionEmitter<Child> {
        let cell = cell.clone();
        ActionEmitter::from_sink(Arc::new(move |action| {
            // The mapping and the parent emitter are cloned out before being
            // called so that a user mapping can never observe this cell
            // mid-borrow.
            let (map, parent) = {
                let cell = cell.borrow();
                (cell.map.clone(), cell.parent.clone())
            };
            parent.emit(map(action))
        }))
    }
}

struct MapActionState<Child: 'static, Parent: 'static> {
    child: Mounted<Child>,
    cell: Rc<RefCell<MapActionCell<Child, Parent>>>,
    /// Emitter handed to the child subtree. Allocated at mount, never replaced.
    composed: ActionEmitter<Child>,
}

impl<Child: 'static, Parent: 'static> MountedState<Parent> for MapActionState<Child, Parent> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Maps the child vocabulary out on the way back up.
    ///
    /// The child is a `Mounted<Child>`, so it cannot be reported through
    /// `visit_children`; this is the mapping half of that boundary.
    fn route(
        &mut self,
        action: &mut Option<RoutedComponentAction>,
        context: &mut RouteContext<'_>,
    ) -> Result<Vec<Parent>, UiError> {
        let actions = self.child.route(action, context)?;
        let map = self.cell.borrow().map.clone();
        Ok(actions.into_iter().map(|action| map(action)).collect())
    }

    fn rebuild_component(
        &mut self,
        target: u64,
        context: &mut RebuildContext<'_>,
    ) -> Result<bool, UiError> {
        self.child.rebuild_component(target, context)
    }
}

impl<Child: 'static, Parent: 'static> ViewNode<Parent> for MapActionView<Child, Parent> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Parent>,
    ) -> Result<Mounted<Parent>, UiError> {
        let cell = Rc::new(RefCell::new(MapActionCell {
            map: self.map,
            parent: context.emitter(),
        }));
        let composed = MapActionCell::compose(&cell);
        let parent = context.parent();
        let child = context.scoped(parent, &composed).build_child(self.child)?;
        let node = child.node();
        Ok(Mounted::new(
            node,
            MapActionState {
                child,
                cell,
                composed,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Parent>,
        context: &mut ViewContext<'_, Parent>,
    ) -> Result<(), UiError> {
        let emitter = context.emitter();
        let parent = context.parent();
        let state = mounted.state_mut::<MapActionState<Child, Parent>>()?;
        {
            let mut cell = state.cell.borrow_mut();
            cell.map = self.map;
            if !cell.parent.ptr_eq(&emitter) {
                cell.parent = emitter;
            }
        }
        let composed = state.composed.clone();
        context
            .scoped(parent, &composed)
            .rebuild_child(&mut state.child, self.child)?;
        let node = state.child.node();
        mounted.set_node(node);
        Ok(())
    }
}
