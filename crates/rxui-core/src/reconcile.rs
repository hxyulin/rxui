//! Keyed reconciliation from lightweight descriptions into retained nodes.

use std::collections::HashMap;

use astrelis_core::{color::Color, geometry::LogicalSize};
use rxui_tree::{
    ActionBox, BoxElement, Flex, Frame, Label, NodeHandle, NodeId, SemanticData, SemanticRole,
};

use crate::{
    App, Element, EntityId, Key, RoutedHandler,
    diagnostics::ViewStats,
    element::{ElementKind, EmbeddedEntity},
};

/// Strategy chosen for one child-list reconciliation pass.
enum ChildStrategy {
    /// Pair new elements with retained ones by position.
    ///
    /// Used for unkeyed lists, and for keyed lists whose length and key order
    /// are both unchanged - which is the overwhelmingly common case, and the one
    /// that used to build and throw away a `HashMap` per container per frame.
    Positional,
    /// Match retained children by key through the reusable index.
    Remap,
}

pub(crate) enum MountedKind {
    Flex {
        handle: NodeHandle<Flex>,
        children: MountedChildren,
    },
    Label(NodeHandle<Label>),
    Button {
        surface: NodeHandle<ActionBox<RoutedHandler>>,
        label: NodeHandle<Label>,
    },
    Entity(EntityId),
    Vacant,
}

pub(crate) struct Mounted {
    key: Option<Key>,
    node: NodeId,
    kind: MountedKind,
}

impl Mounted {
    pub(crate) fn node(&self) -> NodeId {
        self.node
    }

    pub(crate) fn forget(self, app: &mut App, remove_node: bool) {
        match self.kind {
            MountedKind::Flex { children, .. } => children.forget(app),
            MountedKind::Entity(id) => app.unregister_renderer(id),
            MountedKind::Label(_) | MountedKind::Button { .. } | MountedKind::Vacant => {}
        }
        if remove_node && app.tree.contains(self.node) {
            app.tree.remove(self.node);
        }
    }
}

pub(crate) fn reconcile_root(
    mounted: &mut Option<Mounted>,
    element: Element,
    parent: NodeId,
    app: &mut App,
) {
    if let Some(retained) = mounted {
        reconcile_element(retained, element, parent, 0, app);
    } else {
        *mounted = Some(mount_element(element, parent, 0, app));
    }
}

/// One container's reconciled child list plus the scratch space it reuses.
///
/// This is the whole of keyed reconciliation, and a container gets it by
/// owning one of these: [`build`](Self::build) at mount and
/// [`reconcile`](Self::reconcile) on every pass.
///
/// Everything here exists to keep a steady-state frame allocation-free: the
/// mounted list is edited in place rather than rebuilt, the key index and the
/// remap buffer are cleared instead of dropped, and the published child order is
/// remembered so an unchanged order is never handed to the engine again.
pub(crate) struct MountedChildren {
    mounted: Vec<Mounted>,
    /// Child order most recently published to the engine.
    published: Vec<NodeId>,
    /// Previous children held during a keyed remap.
    scratch: Vec<Option<Mounted>>,
    /// Key index reused by validation and by the keyed remap.
    index: HashMap<Key, usize>,
}

impl MountedChildren {
    /// Creates an empty child list.
    pub(crate) fn new() -> Self {
        Self {
            mounted: Vec::new(),
            published: Vec::new(),
            scratch: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Mounts an initial child list below `parent`.
    ///
    /// The engine appends children in order, so the published order is recorded
    /// rather than set: there is nothing to reorder yet.
    fn build(&mut self, elements: Vec<Element>, parent: NodeId, app: &mut App) {
        let _ = self.validate(&elements);
        self.mounted.reserve(elements.len());
        self.published.reserve(elements.len());
        for element in elements {
            let built = mount_element(element, parent, usize::MAX, app);
            self.published.push(built.node());
            self.mounted.push(built);
        }
    }

    /// Reconciles a replacement child list into retained state.
    ///
    /// Panics when the sequence is partially keyed or carries a duplicate key,
    /// which are authoring mistakes rather than recoverable conditions: both
    /// silently lose retained identity on the next insertion.
    fn reconcile(&mut self, elements: Vec<Element>, parent: NodeId, app: &mut App) {
        ViewStats::record_container_reconciled();
        match self.validate(&elements) {
            ChildStrategy::Positional => self.reconcile_positional(elements, parent, app),
            ChildStrategy::Remap => self.reconcile_keyed(elements, parent, app),
        }
        self.publish(parent, app);
    }

    /// Checks keying and picks a strategy in a single scan.
    ///
    /// One pass rejects partially keyed sequences, rejects duplicate keys, and
    /// decides whether the new keys already line up with the retained ones. The
    /// duplicate check reuses `index` instead of allocating a `HashSet` per
    /// container per frame.
    fn validate(&mut self, elements: &[Element]) -> ChildStrategy {
        let keyed = elements
            .first()
            .is_some_and(|element| element.key.is_some());
        if !keyed {
            assert!(
                !elements.iter().any(|element| element.key.is_some()),
                "dynamic element sequences must key every child or no children"
            );
            return ChildStrategy::Positional;
        }
        self.index.clear();
        self.index.reserve(elements.len());
        let mut aligned = self.mounted.len() == elements.len();
        for (position, element) in elements.iter().enumerate() {
            let key = element
                .key
                .as_ref()
                .expect("dynamic element sequences must key every child or no children");
            assert!(
                self.index.insert(key.clone(), position).is_none(),
                "duplicate element key `{key}`"
            );
            if aligned && self.mounted[position].key.as_ref() != Some(key) {
                aligned = false;
            }
        }
        if aligned {
            ChildStrategy::Positional
        } else {
            ChildStrategy::Remap
        }
    }

    fn reconcile_positional(&mut self, elements: Vec<Element>, parent: NodeId, app: &mut App) {
        let kept = elements.len();
        for (position, element) in elements.into_iter().enumerate() {
            if let Some(retained) = self.mounted.get_mut(position) {
                reconcile_element(retained, element, parent, position, app);
            } else {
                self.mounted
                    .push(mount_element(element, parent, position, app));
            }
        }
        while self.mounted.len() > kept {
            self.mounted
                .pop()
                .expect("length was checked")
                .forget(app, true);
        }
    }

    fn reconcile_keyed(&mut self, elements: Vec<Element>, parent: NodeId, app: &mut App) {
        // Re-purpose the index built by `validate` as retained key -> slot.
        self.index.clear();
        self.scratch.clear();
        for retained in self.mounted.drain(..) {
            let key = retained.key.clone().expect("validated keyed sequence");
            self.index.insert(key, self.scratch.len());
            self.scratch.push(Some(retained));
        }
        for element in elements {
            let key = element.key.as_ref().expect("validated keyed sequence");
            let retained = self
                .index
                .get(key)
                .copied()
                .and_then(|slot| self.scratch[slot].take());
            match retained {
                Some(mut retained) => {
                    let position = self.mounted.len();
                    reconcile_element(&mut retained, element, parent, position, app);
                    self.mounted.push(retained);
                }
                None => self
                    .mounted
                    .push(mount_element(element, parent, usize::MAX, app)),
            }
        }
        for slot in &mut self.scratch {
            if let Some(extra) = slot.take() {
                extra.forget(app, true);
            }
        }
        self.scratch.clear();
    }

    /// Publishes the child order, but only when it actually moved.
    ///
    /// The engine also diffs what it is handed. This memo remains because that
    /// diff is O(children), and because `set_children_calls` counts real
    /// structural change rather than calls the retained engine discarded.
    fn publish(&mut self, parent: NodeId, app: &mut App) {
        if self.published.len() == self.mounted.len()
            && self
                .published
                .iter()
                .zip(&self.mounted)
                .all(|(node, element)| *node == element.node())
        {
            return;
        }
        self.published.clear();
        self.published
            .extend(self.mounted.iter().map(Mounted::node));
        ViewStats::record_set_children();
        app.tree.set_children(parent, &self.published);
    }

    fn forget(self, app: &mut App) {
        for mounted in self.mounted {
            mounted.forget(app, false);
        }
    }
}

pub(crate) fn mount_element(
    element: Element,
    parent: NodeId,
    position: usize,
    app: &mut App,
) -> Mounted {
    ViewStats::record_node_built();
    let key = element.key;
    match element.kind {
        ElementKind::Flex {
            axis,
            gap,
            children: elements,
        } => {
            let handle = app.tree.insert_child_at(
                parent,
                position,
                Flex {
                    axis,
                    gap,
                    ..Flex::default()
                },
            );
            let mut children = MountedChildren::new();
            children.build(elements, handle.id(), app);
            Mounted {
                key,
                node: handle.id(),
                kind: MountedKind::Flex { handle, children },
            }
        }
        ElementKind::Label { text } => {
            let handle = app.tree.insert_child_at(parent, position, Label::new(text));
            Mounted {
                key,
                node: handle.id(),
                kind: MountedKind::Label(handle),
            }
        }
        ElementKind::Button { text, on_click } => {
            let frame = app.tree.insert_child_at(parent, position, Frame::default());
            let surface = app
                .tree
                .append(frame.id(), ActionBox::new(button_surface(&text), on_click));
            let label = app.tree.append(frame.id(), Label::new(text));
            Mounted {
                key,
                node: frame.id(),
                kind: MountedKind::Button { surface, label },
            }
        }
        ElementKind::Entity(entity) => mount_entity(key, entity, parent, position, app),
    }
}

fn mount_entity(
    key: Option<Key>,
    entity: EmbeddedEntity,
    parent: NodeId,
    position: usize,
    app: &mut App,
) -> Mounted {
    let boundary = app.tree.insert_child_at(parent, position, Frame::default());
    let id = entity.cell.id;
    app.register_renderer(entity, boundary);
    Mounted {
        key,
        node: boundary.id(),
        kind: MountedKind::Entity(id),
    }
}

fn reconcile_element(
    retained: &mut Mounted,
    element: Element,
    parent: NodeId,
    position: usize,
    app: &mut App,
) {
    let key = element.key.clone();
    match (&mut retained.kind, element.kind) {
        (
            MountedKind::Flex { handle, children },
            ElementKind::Flex {
                axis,
                gap,
                children: elements,
            },
        ) => {
            ViewStats::record_node_rebuilt();
            app.tree.flex_mut(*handle).set_axis(axis);
            app.tree.flex_mut(*handle).set_gap(gap);
            children.reconcile(elements, handle.id(), app);
            retained.key = key;
        }
        (MountedKind::Label(handle), ElementKind::Label { text }) => {
            ViewStats::record_node_rebuilt();
            app.tree.label_mut(*handle).set_text(text);
            retained.key = key;
        }
        (MountedKind::Button { surface, label, .. }, ElementKind::Button { text, on_click }) => {
            ViewStats::record_node_rebuilt();
            app.tree.edit(*surface).set_surface(button_surface(&text));
            app.tree.edit(*surface).set_action(on_click);
            app.tree.label_mut(*label).set_text(text);
            retained.key = key;
        }
        (MountedKind::Entity(id), ElementKind::Entity(entity)) if *id == entity.cell.id => {
            // Entity boundaries are update-isolation gates. Encountering an
            // unchanged child handle does not reconcile or count its subtree.
            retained.key = key;
        }
        (_, kind) => {
            let replacement = Element { key, kind };
            let old = Mounted {
                key: retained.key.take(),
                node: retained.node,
                kind: std::mem::replace(&mut retained.kind, MountedKind::Vacant),
            };
            old.forget(app, true);
            *retained = mount_element(replacement, parent, position, app);
        }
    }
}

fn button_surface(text: &str) -> BoxElement {
    BoxElement {
        size: LogicalSize::ZERO,
        color: Color::from_srgb8(60, 60, 64, 255),
        semantics: Some(SemanticData {
            role: SemanticRole::Button,
            label: text.to_owned(),
            ..SemanticData::default()
        }),
        interactive: true,
    }
}
