//! Containers and the sized, colored box every panel is built from.

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize},
};
use astrelis_ui_next::{
    Axis, BoxElement, Flex, Invalidation, NodeHandle, Scroll, ScrollAxis, SemanticData, SplitPane,
    Stack, UiError,
};
use std::sync::Arc;

use crate::{
    ColorRole, ContainerStyle, Space, StackStyle, View,
    view::{
        AnyView, IntoChildren, MapCell, Mounted, MountedChildren, ViewContext, ViewNode,
        container_mounted_state, leaf_mounted_state,
    },
};
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
/// Creates a vertical flex view.
pub fn column<Action: 'static>(children: impl IntoChildren<Action>) -> AnyView<Action> {
    column_with(ContainerStyle::default(), children)
}

/// Creates a horizontal flex view.
pub fn row<Action: 'static>(children: impl IntoChildren<Action>) -> AnyView<Action> {
    row_with(ContainerStyle::default(), children)
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

impl<Action: 'static> ViewNode<Action> for BoxView {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let color = context.theme().color(self.role);
        let handle = context.append(BoxElement {
            size: self.size,
            color,
            semantics: self.semantics.clone(),
            interactive: self.interactive,
        })?;
        Ok(Mounted::new(
            handle.id(),
            BoxState {
                handle,
                size: self.size,
                color,
                semantics: self.semantics,
                interactive: self.interactive,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let color = context.theme().color(self.role);
        let state = mounted.state_mut::<BoxState>()?;
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
        context.ui().update(state.handle, invalidation, |element| {
            element.size = size;
            element.color = color;
            element.semantics = semantics;
            element.interactive = interactive;
        })?;
        state.size = size;
        state.color = color;
        state.semantics = self.semantics;
        state.interactive = interactive;
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

impl<Action: 'static> ViewNode<Action> for SplitPaneView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let resize = MapCell::new(self.on_resize, context.emitter());
        let emit = resize.emitter();
        let handle = context.append(SplitPane::new(self.axis, self.ratio, move |ratio| {
            emit(ratio)
        }))?;
        let mut children = MountedChildren::new();
        children.build(self.children, &mut context.child(handle.id()))?;
        Ok(Mounted::new(
            handle.id(),
            SplitPaneState {
                handle,
                axis: self.axis,
                ratio: self.ratio,
                resize,
                children,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let emitter = context.emitter();
        let state = mounted.state_mut::<SplitPaneState<Action>>()?;
        state.resize.update(self.on_resize, &emitter);
        // Guarded like every other container. This was the one view that asked
        // for `LAYOUT_ALL` on every pass whether or not the divider moved.
        if state.axis != self.axis || state.ratio != self.ratio {
            let axis = self.axis;
            let ratio = self.ratio;
            context
                .ui()
                .update(state.handle, Invalidation::LAYOUT_ALL, |split| {
                    split.axis = axis;
                    split.ratio = ratio;
                })?;
            state.axis = axis;
            state.ratio = ratio;
        }
        let parent = state.handle.id();
        state
            .children
            .reconcile(self.children, &mut context.child(parent))
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

impl<Action: 'static> ViewNode<Action> for ScrollView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let handle = context.append(Scroll::new(self.axis))?;
        let axis = self.axis;
        let offset = self.offset;
        // Both fields feed `Scroll::layout`, which measures children against the
        // axis and places them at the offset.
        context
            .ui()
            .update(handle, Invalidation::LAYOUT_ALL, |scroll| {
                scroll.axis = axis;
                scroll.offset = offset;
            })?;
        let mut children = MountedChildren::new();
        children.build(self.children, &mut context.child(handle.id()))?;
        Ok(Mounted::new(
            handle.id(),
            ScrollState {
                handle,
                axis,
                requested_offset: offset,
                children,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted.state_mut::<ScrollState<Action>>()?;
        if state.axis != self.axis || state.requested_offset != self.offset {
            let axis = self.axis;
            let offset = self.offset;
            context
                .ui()
                .update(state.handle, Invalidation::LAYOUT_ALL, |scroll| {
                    scroll.axis = axis;
                    scroll.offset = offset;
                })?;
            state.axis = axis;
            state.requested_offset = offset;
        }
        let parent = state.handle.id();
        state
            .children
            .reconcile(self.children, &mut context.child(parent))
    }
}

struct StackState<Action: 'static> {
    handle: NodeHandle<Stack>,
    padding: f32,
    background: Option<Color>,
    children: MountedChildren<Action>,
}

container_mounted_state!(StackState);

impl<Action: 'static> ViewNode<Action> for StackView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let theme = context.theme();
        let padding = theme.space(self.padding);
        let background = self.background.map(|role| theme.color(role));
        let handle = context.append(Stack {
            padding,
            background,
        })?;
        let mut children = MountedChildren::new();
        children.build(self.children, &mut context.child(handle.id()))?;
        Ok(Mounted::new(
            handle.id(),
            StackState {
                handle,
                padding,
                background,
                children,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let theme = context.theme();
        let padding = theme.space(self.padding);
        let background = self.background.map(|role| theme.color(role));
        let state = mounted.state_mut::<StackState<Action>>()?;
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
            context.ui().update(state.handle, invalidation, |stack| {
                stack.padding = padding;
                stack.background = background;
            })?;
            state.padding = padding;
            state.background = background;
        }
        let parent = state.handle.id();
        state
            .children
            .reconcile(self.children, &mut context.child(parent))
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

impl<Action: 'static> ViewNode<Action> for FlexView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let theme = context.theme();
        let gap = theme.space(self.gap);
        let padding = theme.space(self.padding);
        let background = self.background.map(|role| theme.color(role));
        let handle = context.append(Flex {
            axis: self.axis,
            gap,
            padding,
            background,
        })?;
        let mut children = MountedChildren::new();
        children.build(self.children, &mut context.child(handle.id()))?;
        Ok(Mounted::new(
            handle.id(),
            FlexState {
                handle,
                axis: self.axis,
                gap,
                padding,
                background,
                children,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let theme = context.theme();
        let gap = theme.space(self.gap);
        let padding = theme.space(self.padding);
        let background = self.background.map(|role| theme.color(role));
        let state = mounted.state_mut::<FlexState<Action>>()?;
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
            let axis = self.axis;
            context.ui().update(state.handle, invalidation, |flex| {
                flex.axis = axis;
                flex.gap = gap;
                flex.padding = padding;
                flex.background = background;
            })?;
            state.axis = axis;
            state.gap = gap;
            state.padding = padding;
            state.background = background;
        }
        let parent = state.handle.id();
        state
            .children
            .reconcile(self.children, &mut context.child(parent))
    }
}
