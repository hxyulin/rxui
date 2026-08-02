//! Keyed reconciliation from lightweight descriptions into retained nodes.

use std::collections::HashMap;

use astrelis_core::{color::Color, geometry::LogicalSize};
use rxui_tree::{
    Button, Checkbox, Flex, Frame, Label, NodeHandle, NodeId, Scroll, ScrollAxis, Slider,
    SplitPane, TextField,
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
    Button(NodeHandle<Button>),
    Checkbox(NodeHandle<Checkbox>),
    Slider(NodeHandle<Slider>),
    TextField {
        field: NodeHandle<TextField>,
        error: NodeHandle<Label>,
    },
    Scroll {
        handle: NodeHandle<Scroll>,
        children: MountedChildren,
    },
    SplitPane {
        handle: NodeHandle<SplitPane>,
        children: MountedChildren,
    },
    List {
        scroll: NodeHandle<Scroll>,
        content: NodeHandle<Flex>,
        children: MountedChildren,
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

    fn entity_id(&self) -> Option<EntityId> {
        match self.kind {
            MountedKind::Entity(id) => Some(id),
            _ => None,
        }
    }

    pub(crate) fn forget(self, app: &mut App, remove_node: bool) {
        match self.kind {
            MountedKind::Flex { children, .. }
            | MountedKind::Scroll { children, .. }
            | MountedKind::SplitPane { children, .. }
            | MountedKind::List { children, .. } => children.forget(app),
            MountedKind::Entity(id) => app.unregister_renderer(id),
            MountedKind::Label(_)
            | MountedKind::Button(_)
            | MountedKind::Checkbox(_)
            | MountedKind::Slider(_)
            | MountedKind::TextField { .. }
            | MountedKind::Vacant => {}
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
    owner_depth: u32,
    app: &mut App,
) {
    if let Some(retained) = mounted {
        reconcile_element(retained, element, parent, 0, owner_depth, app);
    } else {
        *mounted = Some(mount_element(element, parent, 0, owner_depth, app));
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
    fn build(&mut self, elements: Vec<Element>, parent: NodeId, owner_depth: u32, app: &mut App) {
        let _ = self.validate(&elements);
        self.mounted.reserve(elements.len());
        self.published.reserve(elements.len());
        for element in elements {
            let built = mount_element(element, parent, usize::MAX, owner_depth, app);
            self.published.push(built.node());
            self.mounted.push(built);
        }
    }

    /// Reconciles a replacement child list into retained state.
    ///
    /// Panics when the sequence is partially keyed or carries a duplicate key,
    /// which are authoring mistakes rather than recoverable conditions: both
    /// silently lose retained identity on the next insertion.
    fn reconcile(
        &mut self,
        elements: Vec<Element>,
        parent: NodeId,
        owner_depth: u32,
        app: &mut App,
    ) {
        ViewStats::record_container_reconciled();
        match self.validate(&elements) {
            ChildStrategy::Positional => {
                self.reconcile_positional(elements, parent, owner_depth, app)
            }
            ChildStrategy::Remap => self.reconcile_keyed(elements, parent, owner_depth, app),
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

    fn reconcile_positional(
        &mut self,
        elements: Vec<Element>,
        parent: NodeId,
        owner_depth: u32,
        app: &mut App,
    ) {
        if elements
            .iter()
            .any(|element| element_entity_id(element).is_some())
            || self
                .mounted
                .iter()
                .any(|mounted| mounted.entity_id().is_some())
        {
            self.reconcile_positional_with_entities(elements, parent, owner_depth, app);
            return;
        }
        let kept = elements.len();
        for (position, element) in elements.into_iter().enumerate() {
            if let Some(retained) = self.mounted.get_mut(position) {
                reconcile_element(retained, element, parent, position, owner_depth, app);
            } else {
                self.mounted
                    .push(mount_element(element, parent, position, owner_depth, app));
            }
        }
        while self.mounted.len() > kept {
            self.mounted
                .pop()
                .expect("length was checked")
                .forget(app, true);
        }
    }

    /// Preserves entity identity when an unkeyed stateful collection shifts.
    fn reconcile_positional_with_entities(
        &mut self,
        elements: Vec<Element>,
        parent: NodeId,
        owner_depth: u32,
        app: &mut App,
    ) {
        self.scratch.clear();
        self.scratch.extend(self.mounted.drain(..).map(Some));

        for (position, element) in elements.into_iter().enumerate() {
            let entity = element_entity_id(&element);
            let retained_slot = if let Some(id) = entity {
                self.scratch.iter().position(|slot| {
                    slot.as_ref()
                        .and_then(Mounted::entity_id)
                        .is_some_and(|retained| retained == id)
                })
            } else {
                self.scratch
                    .get(position)
                    .filter(|slot| {
                        slot.as_ref()
                            .is_some_and(|mounted| mounted.entity_id().is_none())
                    })
                    .map(|_| position)
            };

            if let Some(slot) = retained_slot {
                if entity.is_some() && slot != position {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "rxui: unkeyed stateful child shifted from position {slot} to {position}; add .key(...) to make reorder intent explicit"
                    );
                }
                let mut retained = self.scratch[slot]
                    .take()
                    .expect("selected retained slot must be occupied");
                reconcile_element(&mut retained, element, parent, position, owner_depth, app);
                self.mounted.push(retained);
            } else {
                self.mounted
                    .push(mount_element(element, parent, usize::MAX, owner_depth, app));
            }
        }

        for slot in &mut self.scratch {
            if let Some(extra) = slot.take() {
                extra.forget(app, true);
            }
        }
        self.scratch.clear();
    }

    fn reconcile_keyed(
        &mut self,
        elements: Vec<Element>,
        parent: NodeId,
        owner_depth: u32,
        app: &mut App,
    ) {
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
                    reconcile_element(&mut retained, element, parent, position, owner_depth, app);
                    self.mounted.push(retained);
                }
                None => {
                    self.mounted
                        .push(mount_element(element, parent, usize::MAX, owner_depth, app))
                }
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
    owner_depth: u32,
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
            children.build(elements, handle.id(), owner_depth, app);
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
        ElementKind::Button {
            text,
            enabled,
            on_click,
        } => {
            let action = on_click.expect("button interactions require .on_click(...)");
            let handle = app
                .tree
                .insert_child_at(parent, position, make_button(text, action));
            app.tree.set_enabled(handle.id(), enabled);
            Mounted {
                key,
                node: handle.id(),
                kind: MountedKind::Button(handle),
            }
        }
        ElementKind::Checkbox {
            label,
            checked,
            on_toggle,
        } => {
            let handler = on_toggle.expect("checkbox interactions require .on_toggle(...)");
            let handle = app.tree.insert_child_at(
                parent,
                position,
                Checkbox::new(label, checked, move |value| Box::new(handler.with(value))),
            );
            Mounted {
                key,
                node: handle.id(),
                kind: MountedKind::Checkbox(handle),
            }
        }
        ElementKind::Slider {
            label,
            value,
            range,
            step,
            on_change,
        } => {
            let handler = on_change.expect("slider interactions require .on_change(...)");
            let mut slider = Slider::new(label, value, range, move |value| {
                Box::new(handler.with(value))
            });
            slider.step = step;
            let handle = app.tree.insert_child_at(parent, position, slider);
            Mounted {
                key,
                node: handle.id(),
                kind: MountedKind::Slider(handle),
            }
        }
        ElementKind::TextField {
            label,
            text,
            error,
            on_input,
            on_commit,
        } => {
            let container = app.tree.insert_child_at(
                parent,
                position,
                Flex {
                    gap: 3.0,
                    ..Flex::default()
                },
            );
            let mut element = TextField::new(label, text);
            if let Some(handler) = on_input {
                element = element.on_changed_factory(move |value| Box::new(handler.with(value)));
            }
            if let Some(handler) = on_commit {
                element = element.on_submitted_factory(move |value| Box::new(handler.with(value)));
            }
            let field = app.tree.append(container.id(), element);
            let error = app
                .tree
                .append(container.id(), Label::new(error.unwrap_or_default()));
            Mounted {
                key,
                node: container.id(),
                kind: MountedKind::TextField { field, error },
            }
        }
        ElementKind::Scroll {
            axis,
            offset,
            child,
            on_scroll,
        } => {
            let mut element = Scroll::new(axis);
            element.offset = offset;
            if let Some(handler) = on_scroll {
                element = element.on_scrolled_factory(move |value| Box::new(handler.with(value)));
            }
            let handle = app.tree.insert_child_at(parent, position, element);
            let mut children = MountedChildren::new();
            children.build(
                child.into_iter().map(|child| *child).collect(),
                handle.id(),
                owner_depth,
                app,
            );
            Mounted {
                key,
                node: handle.id(),
                kind: MountedKind::Scroll { handle, children },
            }
        }
        ElementKind::SplitPane {
            axis,
            ratio,
            children: elements,
            on_change,
        } => {
            let handler = on_change.expect("split pane interactions require .on_change(...)");
            let handle = app.tree.insert_child_at(
                parent,
                position,
                SplitPane::new(axis, ratio, move |value| Box::new(handler.with(value))),
            );
            let mut children = MountedChildren::new();
            children.build(elements, handle.id(), owner_depth, app);
            Mounted {
                key,
                node: handle.id(),
                kind: MountedKind::SplitPane { handle, children },
            }
        }
        ElementKind::List {
            children: elements,
            offset,
            on_scroll,
        } => {
            assert_keyed_list(&elements);
            let mut element = Scroll::new(ScrollAxis::Vertical);
            element.offset = offset;
            if let Some(handler) = on_scroll {
                element = element.on_scrolled_factory(move |value| Box::new(handler.with(value)));
            }
            let scroll = app.tree.insert_child_at(parent, position, element);
            let content = app.tree.append(scroll.id(), Flex::default());
            let mut children = MountedChildren::new();
            children.build(elements, content.id(), owner_depth, app);
            Mounted {
                key,
                node: scroll.id(),
                kind: MountedKind::List {
                    scroll,
                    content,
                    children,
                },
            }
        }
        ElementKind::Entity(entity) => {
            mount_entity(key, entity, parent, position, owner_depth, app)
        }
    }
}

fn mount_entity(
    key: Option<Key>,
    entity: EmbeddedEntity,
    parent: NodeId,
    position: usize,
    owner_depth: u32,
    app: &mut App,
) -> Mounted {
    let boundary = app.tree.insert_child_at(parent, position, Frame::default());
    let id = entity.cell.id;
    app.set_entity_depth(&entity.cell, owner_depth.saturating_add(1));
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
    owner_depth: u32,
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
            children.reconcile(elements, handle.id(), owner_depth, app);
            retained.key = key;
        }
        (MountedKind::Label(handle), ElementKind::Label { text }) => {
            ViewStats::record_node_rebuilt();
            app.tree.label_mut(*handle).set_text(text);
            retained.key = key;
        }
        (
            MountedKind::Button(handle),
            ElementKind::Button {
                text,
                enabled,
                on_click,
            },
        ) => {
            ViewStats::record_node_rebuilt();
            app.tree.button_mut(*handle).set_label(text);
            app.tree.set_enabled(handle.id(), enabled);
            let action = on_click.expect("button interactions require .on_click(...)");
            app.tree
                .button_mut(*handle)
                .set_action(move || Box::new(action.clone()));
            retained.key = key;
        }
        (
            MountedKind::Checkbox(handle),
            ElementKind::Checkbox {
                label,
                checked,
                on_toggle,
            },
        ) => {
            ViewStats::record_node_rebuilt();
            app.tree.checkbox_mut(*handle).set_label(label);
            app.tree.checkbox_mut(*handle).set_checked(checked);
            let handler = on_toggle.expect("checkbox interactions require .on_toggle(...)");
            app.tree
                .checkbox_mut(*handle)
                .set_change_action(move |value| Box::new(handler.with(value)));
            retained.key = key;
        }
        (
            MountedKind::Slider(handle),
            ElementKind::Slider {
                label,
                value,
                range,
                step,
                on_change,
            },
        ) => {
            ViewStats::record_node_rebuilt();
            app.tree.slider_mut(*handle).set_label(label);
            app.tree.slider_mut(*handle).set_range(range);
            app.tree.slider_mut(*handle).set_value(value);
            app.tree.slider_mut(*handle).set_step(step);
            let handler = on_change.expect("slider interactions require .on_change(...)");
            app.tree
                .slider_mut(*handle)
                .set_change_action(move |value| Box::new(handler.with(value)));
            retained.key = key;
        }
        (
            MountedKind::TextField {
                field,
                error: issue,
                ..
            },
            ElementKind::TextField {
                label,
                text,
                error,
                on_input,
                on_commit,
            },
        ) => {
            ViewStats::record_node_rebuilt();
            app.tree.text_field_mut(*field).set_label(label);
            app.tree.text_field_mut(*field).set_text(text);
            app.tree
                .label_mut(*issue)
                .set_text(error.unwrap_or_default());
            if let Some(handler) = on_input {
                app.tree
                    .text_field_mut(*field)
                    .set_change_action(move |value| Box::new(handler.with(value)));
            }
            if let Some(handler) = on_commit {
                app.tree
                    .text_field_mut(*field)
                    .set_submit_action(move |value| Box::new(handler.with(value)));
            }
            retained.key = key;
        }
        (
            MountedKind::Scroll { handle, children },
            ElementKind::Scroll {
                axis,
                offset,
                child,
                on_scroll,
            },
        ) => {
            ViewStats::record_node_rebuilt();
            app.tree.scroll_mut(*handle).set_axis(axis);
            app.tree.scroll_mut(*handle).set_offset(offset);
            if let Some(handler) = on_scroll {
                app.tree
                    .scroll_mut(*handle)
                    .set_scrolled_factory(move |value| Box::new(handler.with(value)));
            }
            children.reconcile(
                child.into_iter().map(|child| *child).collect(),
                handle.id(),
                owner_depth,
                app,
            );
            retained.key = key;
        }
        (
            MountedKind::SplitPane { handle, children },
            ElementKind::SplitPane {
                axis,
                ratio,
                children: elements,
                on_change,
            },
        ) => {
            ViewStats::record_node_rebuilt();
            app.tree.split_pane_mut(*handle).set_axis(axis);
            app.tree.split_pane_mut(*handle).set_ratio(ratio);
            let handler = on_change.expect("split pane interactions require .on_change(...)");
            app.tree
                .split_pane_mut(*handle)
                .set_change_action(move |value| Box::new(handler.with(value)));
            children.reconcile(elements, handle.id(), owner_depth, app);
            retained.key = key;
        }
        (
            MountedKind::List {
                scroll,
                content,
                children,
            },
            ElementKind::List {
                children: elements,
                offset,
                on_scroll,
            },
        ) => {
            ViewStats::record_node_rebuilt();
            assert_keyed_list(&elements);
            app.tree.scroll_mut(*scroll).set_offset(offset);
            if let Some(handler) = on_scroll {
                app.tree
                    .scroll_mut(*scroll)
                    .set_scrolled_factory(move |value| Box::new(handler.with(value)));
            }
            children.reconcile(elements, content.id(), owner_depth, app);
            retained.key = key;
        }
        (MountedKind::Entity(id), ElementKind::Entity(entity)) if *id == entity.cell.id => {
            // Entity boundaries are update-isolation gates. Encountering an
            // unchanged child handle does not reconcile or count its subtree.
            app.set_entity_depth(&entity.cell, owner_depth.saturating_add(1));
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
            *retained = mount_element(replacement, parent, position, owner_depth, app);
        }
    }
}

fn make_button(text: String, action: RoutedHandler) -> Button {
    Button::with_action_factory(
        text.clone(),
        LogicalSize::new((text.chars().count() as f32 * 9.0 + 20.0).max(32.0), 28.0),
        Color::from_srgb8(60, 60, 64, 255),
        Color::from_srgb8(82, 82, 88, 255),
        move || Box::new(action.clone()),
    )
}

fn element_entity_id(element: &Element) -> Option<EntityId> {
    match &element.kind {
        ElementKind::Entity(entity) => Some(entity.cell.id),
        _ => None,
    }
}

fn assert_keyed_list(elements: &[Element]) {
    assert!(
        elements.iter().all(|element| element.key.is_some()),
        "list children must all have stable .key(...) identities"
    );
}
