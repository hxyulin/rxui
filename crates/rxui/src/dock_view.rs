//! Dock tree composition using existing controlled tabs and split controls.
use crate::{
    DockEvent, DockNode, DockTree, Element, IntoElement, Key, Listener, ResizeEvent, TabActivation,
    TabCloseEvent, TabContentPolicy, TabSelectEvent, dock::DockNodeId, split_column, split_row,
    stack, tab, tabs,
};
use std::collections::HashMap;
use taffy::prelude::{AlignItems, JustifyItems, TaffyAuto, fr, length, minmax};

/// One panel's application-provided title and content. Its stable key lives in
/// DockTree; content can be any IntoElement, including a strong document Entity.
#[must_use]
pub struct DockPanel {
    title: String,
    content: Element,
    closable: bool,
}
/// Describes content for a panel key supplied by the dock resolver.
pub fn dock_panel(title: impl Into<String>, content: impl IntoElement) -> DockPanel {
    DockPanel {
        title: title.into(),
        content: content.into_element(),
        closable: false,
    }
}
impl DockPanel {
    /// Shows a close control when the Dock has an on_event listener. Close only
    /// proposes removal; the application decides when to apply it and dispose data.
    pub fn closable(mut self, value: bool) -> Self {
        self.closable = value;
        self
    }
}
#[derive(Clone)]
pub(crate) struct Properties {
    pub min_pane_size: [f32; 2],
    pub divider_size: f32,
    pub drag_threshold: f32,
    pub draggable: bool,
    pub show_preview: bool,
    pub listener: Option<Listener<DockEvent>>,
}
impl Properties {
    pub(crate) fn valid(&self) -> bool {
        self.min_pane_size.iter().all(|n| n.is_finite() && *n >= 0.)
            && self.divider_size.is_finite()
            && self.divider_size > 0.
            && self.drag_threshold.is_finite()
            && self.drag_threshold >= 0.
    }
}
#[derive(Clone)]
pub(crate) enum Metadata {
    Root(Properties),
    Group(DockNodeId),
}
impl Metadata {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Root(p) => p.valid(),
            Self::Group(_) => true,
        }
    }
}
/// Owned dock description. Takes a tree snapshot at construction; it never mutates
/// that model. A resolver runs once per panel in tree order and must describe every
/// live key, including inactive panels. Only selected descriptions mount under
/// MountSelected. Read document data through cx in the resolver to track dependencies.
///
/// ```
/// use rxui::prelude::*;
/// struct Workspace { layout: DockTree }
/// impl View for Workspace {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         dock(&self.layout, |key| dock_panel(format!("{key:?}"), label("Content")))
///             .on_event(cx.listener(|s, e: &DockEvent, _| {
///                 let _ = s.layout.apply(e); // stale/deferred proposals may be rejected
///             }))
///     }
/// }
/// ```
#[must_use = "attach the dock to a parent or return it from View::view"]
pub struct Dock {
    root: Element,
    tree: Option<DockTree>,
    panels: HashMap<Key, DockPanel>,
    listener: Option<Listener<DockEvent>>,
    policy: TabContentPolicy,
    activation: TabActivation,
    props: Properties,
}
/// Composes an application-owned tree with content supplied by stable panel key.
/// Default panes have 96-unit minima and 8-unit dividers. Header dragging is enabled
/// with an event listener and a 6-unit movement threshold. The dock fills its bounded
/// parent; undersized viewports clip panes at their recursively combined minima.
pub fn dock(tree: &DockTree, mut resolve: impl FnMut(&Key) -> DockPanel) -> Dock {
    fn collect(
        node: &DockNode,
        resolve: &mut impl FnMut(&Key) -> DockPanel,
        panels: &mut HashMap<Key, DockPanel>,
    ) {
        match node {
            DockNode::Tabs(n) => {
                for key in n.panels() {
                    panels.insert(key.clone(), resolve(key));
                }
            }
            DockNode::Split(n) => {
                collect(n.first(), resolve, panels);
                collect(n.second(), resolve, panels);
            }
        }
    }
    let mut panels = HashMap::new();
    collect(tree.root(), &mut resolve, &mut panels);
    Dock {
        root: stack()
            .fill_width()
            .fill_height()
            .flex_grow(1.)
            .flex_basis(0.)
            .min_width(0.)
            .min_height(0.)
            .clip()
            .layout(|s| {
                s.grid_template_rows = vec![minmax(length(0.), fr(1.))];
                s.grid_template_columns = vec![minmax(length(0.), fr(1.))];
                s.align_items = Some(AlignItems::STRETCH);
                s.justify_items = Some(JustifyItems::STRETCH);
            }),
        tree: Some(tree.clone()),
        panels,
        listener: None,
        policy: TabContentPolicy::KeepMounted,
        activation: TabActivation::Automatic,
        props: Properties {
            min_pane_size: [96., 96.],
            divider_size: 8.,
            drag_threshold: 6.,
            draggable: true,
            show_preview: true,
            listener: None,
        },
    }
}
impl Dock {
    /// Receives controlled selection, close, resize and drop proposals with node identity.
    /// Without a listener, dividers and close controls are read-only/unavailable.
    pub fn on_event(mut self, listener: Listener<DockEvent>) -> Self {
        self.listener = Some(listener);
        self
    }
    /// Policy for inactive panels in every tab group. Cross-group moves/topology
    /// reparenting may remount content even with KeepMounted; entities retain data.
    pub fn content_policy(mut self, policy: TabContentPolicy) -> Self {
        self.policy = policy;
        self
    }
    /// Keyboard activation convention for every group.
    pub fn activation(mut self, activation: TabActivation) -> Self {
        self.activation = activation;
        self
    }
    /// Leaf group minima in logical units, combined recursively for nested splits.
    /// Nonfinite/negative values are rejected during Ui preparation.
    pub fn min_pane_size(mut self, width: f32, height: f32) -> Self {
        self.props.min_pane_size = [width, height];
        self
    }
    /// Divider hit thickness. Must be finite and positive; validated during prepare.
    pub fn divider_size(mut self, size: f32) -> Self {
        self.props.divider_size = size;
        self
    }
    /// Enables header dragging within this dock placement. Requires on_event.
    /// Defaults to true; ordinary header clicking/keyboard navigation remains available.
    pub fn draggable(mut self, value: bool) -> Self {
        self.props.draggable = value;
        self
    }
    /// Enables UiPainter's themed drop overlay (default true). Disable it when a
    /// custom painter uses Ui::dock_drag for feedback; gestures/proposals still work.
    pub fn drop_preview(mut self, value: bool) -> Self {
        self.props.show_preview = value;
        self
    }
    /// Movement in logical units before a pressed header becomes a drag (default 6).
    /// Must be finite and nonnegative; validated during Ui preparation.
    pub fn drag_threshold(mut self, distance: f32) -> Self {
        self.props.drag_threshold = distance;
        self
    }
    /// Stable identity of the entire dock within its parent's sibling scope.
    pub fn key(mut self, key: impl Into<Key>) -> Self {
        self.root = self.root.key(key);
        self
    }
    /// Fixed logical dimensions for a standalone dock placement.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.root = self
            .root
            .size(width, height)
            .flex_grow(0.)
            .layout(|s| s.flex_basis = taffy::Dimension::AUTO);
        self
    }
    /// Root width constraint.
    pub fn width(mut self, width: f32) -> Self {
        self.root = self.root.width(width);
        self
    }
    /// Root height constraint.
    pub fn height(mut self, height: f32) -> Self {
        self.root = self.root.height(height);
        self
    }
    /// Fills parent width.
    pub fn fill_width(mut self) -> Self {
        self.root = self.root.fill_width();
        self
    }
    /// Fills parent height.
    pub fn fill_height(mut self) -> Self {
        self.root = self.root.fill_height();
        self
    }
    /// Root flex growth.
    pub fn flex_grow(mut self, factor: f32) -> Self {
        self.root = self.root.flex_grow(factor);
        self
    }
    /// Full root Taffy customization.
    pub fn layout(mut self, configure: impl FnOnce(&mut taffy::Style)) -> Self {
        self.root = self.root.layout(configure);
        self
    }
    fn describe(&mut self, node: &DockNode) -> (Element, [f32; 2]) {
        let id: DockNodeId = node.id();
        match node {
            DockNode::Tabs(n) => {
                let mut group = tabs()
                    .key(id.key())
                    .content_policy(self.policy)
                    .activation(self.activation)
                    .min_width(self.props.min_pane_size[0])
                    .min_height(self.props.min_pane_size[1]);
                if let Some(key) = n.selected() {
                    group = group.selected(key.clone());
                }
                for key in n.panels() {
                    let panel = self.panels.remove(key).expect("resolved dock panel");
                    group = group
                        .tab(tab(key.clone(), panel.title, panel.content).closable(panel.closable));
                }
                if let Some(listener) = &self.listener {
                    group = group
                        .on_select(listener.map_event(move |e: &TabSelectEvent| {
                            DockEvent::Select {
                                group: id,
                                panel: e.key.clone(),
                            }
                        }))
                        .on_close(
                            listener.map_event(move |e: &TabCloseEvent| DockEvent::Close {
                                group: id,
                                panel: e.key.clone(),
                            }),
                        );
                }
                let mut element = group
                    .into_element()
                    .pointer_events(crate::PointerEvents::Block);
                element.input.get_or_insert_with(Default::default).dock =
                    Some(Box::new(Metadata::Group(id)));
                (element, self.props.min_pane_size)
            }
            DockNode::Split(n) => {
                let (first, a) = self.describe(n.first());
                let (second, b) = self.describe(n.second());
                let axis = n.axis().index();
                let mut split = if axis == 0 {
                    split_row(first, second)
                } else {
                    split_column(first, second)
                }
                .fill_width()
                .fill_height()
                .position(n.position())
                .min_first(a[axis])
                .min_second(b[axis])
                .divider_size(self.props.divider_size);
                if let Some(listener) = &self.listener {
                    split = split.on_resize(listener.map_event(move |e: &ResizeEvent| {
                        DockEvent::Resize {
                            split: id,
                            event: *e,
                        }
                    }));
                }
                let mut size = [a[0].max(b[0]), a[1].max(b[1])];
                size[axis] = a[axis] + b[axis] + self.props.divider_size;
                (split.key(id.key()), size)
            }
        }
    }
}
impl IntoElement for Dock {
    fn into_element(mut self) -> Element {
        let tree = self.tree.take().expect("owned dock snapshot");
        let (content, _) = self.describe(tree.root());
        self.props.listener = self.listener;
        self.root.input.get_or_insert_with(Default::default).dock =
            Some(Box::new(Metadata::Root(self.props)));
        self.root.child(content)
    }
}
