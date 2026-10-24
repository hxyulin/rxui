//! Dock tree composition using existing controlled tabs and split controls.
use crate::{
    DockEvent, DockNode, DockTree, Element, IntoElement, Key, Listener, ResizeEvent, TabActivation,
    TabCloseEvent, TabContentPolicy, TabSelectEvent, dock::DockNodeId, split_column, split_row,
    stack, tab, tabs,
};
use taffy::prelude::{AlignItems, JustifyItems, TaffyAuto, fr, length, minmax};

/// Request to open an application-provided menu for a dock header.
#[derive(Clone, Debug)]
pub struct DockContextEvent {
    /// Header's current group.
    pub group: DockNodeId,
    /// Header's panel key; selection is not changed by this request.
    pub panel: Key,
    /// Logical window position suitable for a point-anchored popover.
    pub position: [f32; 2],
}

/// One panel's application-provided title and content. Its stable key lives in
/// DockTree; content can be any IntoElement, including a strong document Entity.
#[must_use]
pub struct DockPanel {
    title: String,
    content: PanelContent,
    closable: bool,
}
/// Describes content for a panel key supplied by the dock resolver.
pub fn dock_panel(title: impl Into<String>, content: impl IntoElement) -> DockPanel {
    DockPanel {
        title: title.into(),
        content: PanelContent::Eager(content.into_element()),
        closable: false,
    }
}
// Eager panels are the existing common path. Keep their Element inline to avoid
// adding one allocation per resolved panel merely to shrink the lazy variant.
#[allow(clippy::large_enum_variant)]
enum PanelContent {
    Eager(Element),
    Lazy(Box<dyn FnOnce() -> Element>),
}
impl PanelContent {
    fn into_element(self, mount: bool) -> Element {
        match self {
            Self::Eager(content) => content,
            Self::Lazy(build) if mount => build(),
            Self::Lazy(_) => stack(),
        }
    }
}
/// Describes a panel whose content is built only when its mounting policy needs it.
/// The resolver still runs eagerly for every live key, providing all header titles.
/// The closure runs during Dock conversion; with MountSelected, inactive closures
/// are dropped without being called. KeepMounted builds every panel on conversion.
/// Read dependencies through cx in the eager resolver and move owned values into
/// this closure; a stored 'static closure cannot borrow the view context.
/// Content may be rebuilt on each view evaluation; this is not a content cache.
pub fn dock_panel_lazy<E: IntoElement>(
    title: impl Into<String>,
    content: impl FnOnce() -> E + 'static,
) -> DockPanel {
    DockPanel {
        title: title.into(),
        content: PanelContent::Lazy(Box::new(move || content().into_element())),
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
    pub surface: Option<crate::DockSurfaceId>,
    pub transfer_scope: Option<Key>,
    pub transfer: Option<Listener<crate::DockTransferEvent>>,
    pub detach: Option<Listener<crate::DockDetachEvent>>,
    pub context_menu: Option<Listener<DockContextEvent>>,
    pub drop_policy: Option<crate::dock_policy::DockDropPolicy>,
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
/// Owned dock description. Takes a composition snapshot at construction; it never mutates
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
    composition: Option<Composition>,
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
                s.align_items = AlignItems::STRETCH;
                s.justify_items = JustifyItems::STRETCH;
            }),
        composition: Some(Composition::resolve(tree.root(), &mut resolve)),
        listener: None,
        policy: TabContentPolicy::KeepMounted,
        activation: TabActivation::Automatic,
        props: Properties {
            surface: None,
            transfer_scope: None,
            transfer: None,
            detach: None,
            context_menu: None,
            drop_policy: None,
            min_pane_size: [96., 96.],
            divider_size: 8.,
            drag_threshold: 6.,
            draggable: true,
            show_preview: true,
            listener: None,
        },
    }
}
// Own exactly the data needed after resolving. Keys move straight into tabs;
// neither a second topology clone nor an all-panel hash table is needed.
enum Composition {
    Tabs {
        id: DockNodeId,
        selected: Option<usize>,
        panels: Vec<(Key, DockPanel)>,
    },
    Split {
        id: DockNodeId,
        axis: crate::Axis,
        position: crate::SplitPosition,
        children: Box<[Composition; 2]>,
    },
}
impl Composition {
    fn resolve(node: &DockNode, resolve: &mut impl FnMut(&Key) -> DockPanel) -> Self {
        match node {
            DockNode::Tabs(n) => Self::Tabs {
                id: n.id(),
                selected: n.panels().iter().position(|key| Some(key) == n.selected()),
                panels: n
                    .panels()
                    .iter()
                    .map(|key| (key.clone(), resolve(key)))
                    .collect(),
            },
            DockNode::Split(n) => Self::Split {
                id: n.id(),
                axis: n.axis(),
                position: n.position(),
                children: Box::new([
                    Self::resolve(n.first(), resolve),
                    Self::resolve(n.second(), resolve),
                ]),
            },
        }
    }
}
impl Dock {
    /// Binds this placement to an application-owned workspace surface.
    pub fn surface(mut self, surface: crate::DockSurfaceId) -> Self {
        self.props.surface = Some(surface);
        self
    }
    /// Allows transfers between explicitly bound surfaces sharing this scope.
    /// Choose a distinct key for each independent workspace in the same Ui.
    pub fn transfer_scope(mut self, scope: impl Into<Key>) -> Self {
        self.props.transfer_scope = Some(scope.into());
        self
    }
    /// Receives proposals to transfer into another surface in the same Ui.
    /// The source application validates and accepts with DockWorkspace::transfer.
    pub fn on_transfer(mut self, listener: Listener<crate::DockTransferEvent>) -> Self {
        self.props.transfer = Some(listener);
        self
    }
    /// Receives release-time proposals outside visible dock surfaces.
    /// Requires a surface binding. The application decides whether to create a
    /// window; position is in source-window logical coordinates.
    pub fn on_detach(mut self, listener: Listener<crate::DockDetachEvent>) -> Self {
        self.props.detach = Some(listener);
        self
    }
    /// Filters proposed drops using application policy before accepting a target.
    pub fn drop_policy(mut self, policy: impl Fn(&crate::DockDropQuery) -> bool + 'static) -> Self {
        self.props.drop_policy = Some(std::rc::Rc::new(policy));
        self
    }
    /// Receives secondary-button or keyboard context-menu requests from headers.
    /// The application supplies controlled menu content and open state.
    pub fn on_context_menu(mut self, listener: Listener<DockContextEvent>) -> Self {
        self.props.context_menu = Some(listener);
        self
    }
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
    /// Enables header dragging within this dock placement. Requires an event
    /// listener, or a surface binding with a transfer/detach listener.
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
        self.root = self.root.size(width, height);
        self.fixed()
    }
    /// Fixed root width. The dock stops growing, so the width also holds when it is
    /// on its flex parent's main axis.
    pub fn width(mut self, width: f32) -> Self {
        self.root = self.root.width(width);
        self.fixed()
    }
    /// Fixed root height. The dock stops growing, so the height also holds when it
    /// is on its flex parent's main axis.
    pub fn height(mut self, height: f32) -> Self {
        self.root = self.root.height(height);
        self.fixed()
    }
    /// Replaces the default zero flex basis, which would override an explicit size
    /// on the main axis, with the root's own size.
    fn fixed(mut self) -> Self {
        self.root = self
            .root
            .flex_grow(0.)
            .layout(|s| s.flex_basis = taffy::Dimension::AUTO);
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
    fn describe(&self, node: Composition) -> (Element, [f32; 2]) {
        match node {
            Composition::Tabs {
                id,
                selected,
                panels,
            } => {
                let mut group = tabs()
                    .key(id.key())
                    .content_policy(self.policy)
                    .activation(self.activation)
                    .min_width(self.props.min_pane_size[0])
                    .min_height(self.props.min_pane_size[1]);
                if let Some(index) = selected {
                    group = group.selected(panels[index].0.clone());
                }
                for (index, (key, panel)) in panels.into_iter().enumerate() {
                    let mount =
                        self.policy == TabContentPolicy::KeepMounted || selected == Some(index);
                    group = group.tab(
                        tab(key, panel.title, panel.content.into_element(mount))
                            .closable(panel.closable),
                    );
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
            Composition::Split {
                id,
                axis,
                position,
                children,
            } => {
                let [first, second] = *children;
                let (first, a) = self.describe(first);
                let (second, b) = self.describe(second);
                let axis = axis.index();
                let mut split = if axis == 0 {
                    split_row(first, second)
                } else {
                    split_column(first, second)
                }
                .fill_width()
                .fill_height()
                .position(position)
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
        let composition = self.composition.take().expect("owned dock snapshot");
        let (content, _) = self.describe(composition);
        self.props.listener = self.listener;
        self.root.input.get_or_insert_with(Default::default).dock =
            Some(Box::new(Metadata::Root(self.props)));
        self.root.child(content)
    }
}
