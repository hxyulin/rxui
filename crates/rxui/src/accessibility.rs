use crate::{
    ElementId, SemanticAction, SemanticRole, TextPosition, TextSelection, Ui, UiError, View,
};
use accesskit::{
    Action, ActionData, ActionRequest, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use unicode_segmentation::UnicodeSegmentation;

/// Cumulative publication counters; warm unchanged snapshots do no node/text work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccessKitStats {
    /// Full tree publications, including reactivation after reset.
    pub full_updates: u64,
    /// Publications of changed nodes/focus after initialization.
    pub delta_updates: u64,
    /// Semantic element nodes examined during changed snapshot construction.
    pub built_nodes: u64,
    /// New grapheme encodings prepared for changed text snapshots.
    pub prepared_text_runs: u64,
}
struct TextRun {
    revision: u64,
    id: NodeId,
    boundaries: Vec<usize>,
    node: Arc<Node>,
}
/// One placement's optional AccessKit translation/publication cache. Use alongside
/// a platform adapter and call update only when accessibility is active. Stable
/// element IDs survive compatible keys; text-run IDs change with text revision so
/// queued selections cannot address a replacement value. No window/GPU is owned.
///
/// Bounds and parent-relative transforms compose into physical window coordinates.
/// Scrolling moves content containers without republishing every descendant's position.
/// Single-line text runs expose grapheme
/// selection units; character geometry and rich text attributes are not yet exposed.
pub struct AccessKitTree {
    placement: Option<u64>,
    next_id: u64,
    ids: HashMap<ElementId, NodeId>,
    reverse: HashMap<NodeId, ElementId>,
    runs: HashMap<ElementId, TextRun>,
    value_only_revisions: HashMap<ElementId, u64>,
    nodes: HashMap<NodeId, Arc<Node>>,
    key: Option<crate::semantics::Key>,
    title: String,
    scale: f32,
    focus: NodeId,
    stats: AccessKitStats,
}
impl Default for AccessKitTree {
    fn default() -> Self {
        Self::new()
    }
}
impl AccessKitTree {
    /// Creates an unbound tree; the first update binds it to one Ui placement.
    pub fn new() -> Self {
        Self {
            placement: None,
            next_id: 1,
            ids: HashMap::new(),
            reverse: HashMap::new(),
            runs: HashMap::new(),
            value_only_revisions: HashMap::new(),
            nodes: HashMap::new(),
            key: None,
            title: String::new(),
            scale: 0.,
            focus: NodeId(0),
            stats: AccessKitStats::default(),
        }
    }
    /// Current publication counters.
    pub fn stats(&self) -> AccessKitStats {
        self.stats
    }
    /// Requires the next update to contain a complete tree, for a new activation
    /// request or after deactivation. Retained identity/encoding tables survive.
    pub fn reset(&mut self) {
        self.key = None;
        self.nodes.clear();
    }
    /// Releases published text/node caches while inactive. Structural identity
    /// remains stable; old text-run identities are retired and never reused.
    pub fn deactivate(&mut self) {
        self.reset();
        self.runs.clear();
        self.value_only_revisions.clear();
        self.reverse.clear();
    }
    fn allocate(&mut self) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("RXUI AccessKit identity exhausted");
        id
    }
    /// Builds a complete initial tree or a changed-node update. Returns None when
    /// semantics are unchanged, including hover/capture/caret-blink-only changes.
    /// Requires successfully prepared geometry and a finite positive DPI scale.
    /// A tree cannot be reused for another Ui placement; create a separate cache.
    pub fn update<T: View>(
        &mut self,
        ui: &Ui<T>,
        title: &str,
        scale: f32,
    ) -> Result<Option<TreeUpdate>, UiError> {
        profiling::scope!("rxui::accessibility_update");
        if !ui.is_prepared() || !scale.is_finite() || scale <= 0. {
            return Err(UiError::InvalidGeometry);
        }
        let key = ui.semantic_key();
        if self.placement.is_some_and(|id| id != key.tree) {
            return Err(UiError::InvalidGeometry);
        }
        self.placement = Some(key.tree);
        if self.key.as_ref() == Some(&key) && self.title == title && self.scale == scale {
            return Ok(None);
        }
        let full = self.key.is_none();
        self.ids.retain(|id, _| ui.contains_element(*id));
        self.runs.retain(|id, _| ui.contains_element(*id));
        self.value_only_revisions
            .retain(|id, _| ui.contains_element(*id));
        let semantics: Vec<_> = ui.semantics().collect();
        let visible: HashSet<_> = semantics.iter().map(|node| node.id).collect();
        for semantic in &semantics {
            if !self.ids.contains_key(&semantic.id) {
                let id = self.allocate();
                self.ids.insert(semantic.id, id);
            }
        }
        let mut next = HashMap::with_capacity(semantics.len() + 1);
        let mut reverse = HashMap::with_capacity(semantics.len());
        let root_id = NodeId(0);
        let mut root = Node::new(Role::Window);
        root.set_label(title);
        let size = key.viewport.unwrap_or([0.; 2]);
        root.set_bounds(rect(
            crate::Bounds {
                x: 0.,
                y: 0.,
                width: size[0],
                height: size[1],
            },
            scale,
        ));
        root.set_children(
            semantics
                .iter()
                .filter(|n| n.parent.is_none() || n.viewport_overlay)
                .map(|n| self.ids[&n.id])
                .collect::<Vec<_>>(),
        );
        root.set_clips_children();
        insert(&self.nodes, &mut next, root_id, root);
        for semantic in semantics {
            self.stats.built_nodes += 1;
            let id = self.ids[&semantic.id];
            reverse.insert(id, semantic.id);
            let mut node = Node::new(role(semantic.role));
            if let Some(size) = semantic.set_size {
                node.set_size_of_set(size);
            }
            if let Some(index) = semantic.position_in_set {
                node.set_position_in_set(index);
            }
            node.set_bounds(rect(
                crate::Bounds {
                    x: 0.,
                    y: 0.,
                    ..semantic.bounds
                },
                scale,
            ));
            let origin = ui.semantic_origin(semantic.id)?;
            node.set_transform(accesskit::Affine::translate((
                origin[0] * f64::from(scale),
                origin[1] * f64::from(scale),
            )));
            if let Some(label) = semantic.label {
                node.set_label(label);
            }
            if semantic.modal {
                node.set_modal();
            }
            if let Some(selected) = semantic.selected {
                node.set_selected(selected);
            }
            if let Some(label) = semantic
                .labelled_by
                .and_then(|id| self.ids.get(&id))
                .filter(|_| semantic.labelled_by.is_some_and(|id| visible.contains(&id)))
            {
                node.set_labelled_by(vec![*label]);
            }
            if let Some(target) = semantic
                .controls
                .and_then(|id| self.ids.get(&id))
                .filter(|_| semantic.controls.is_some_and(|id| visible.contains(&id)))
            {
                node.set_controls(vec![*target]);
            }
            if let Some(axis) = semantic.orientation {
                node.set_orientation(if axis == crate::Axis::Horizontal {
                    accesskit::Orientation::Horizontal
                } else {
                    accesskit::Orientation::Vertical
                });
            }
            if let Some(description) = semantic.description {
                node.set_description(description);
            }
            if semantic.role == SemanticRole::Heading {
                node.set_level(1);
            }
            node.set_children(
                semantic
                    .children
                    .iter()
                    .filter(|id| visible.contains(id))
                    .filter(|id| !ui.is_overlay(**id))
                    .map(|id| self.ids[id])
                    .collect::<Vec<_>>(),
            );
            if semantic.disabled {
                node.set_disabled();
            }
            if semantic.read_only {
                node.set_read_only();
            }
            if semantic.clips_children {
                node.set_clips_children();
            }
            if semantic.focusable {
                node.add_action(Action::Focus);
            }
            if semantic.activatable {
                node.add_action(Action::Click);
            }
            if let Some(range) = semantic.range {
                node.set_numeric_value(f64::from(range.value));
                node.set_min_numeric_value(f64::from(range.min));
                node.set_max_numeric_value(f64::from(range.max));
                node.set_numeric_value_step(f64::from(range.step));
                let horizontal = if semantic.role == SemanticRole::Splitter {
                    range.axis == crate::Axis::Vertical
                } else {
                    range.axis == crate::Axis::Horizontal
                };
                node.set_orientation(if horizontal {
                    accesskit::Orientation::Horizontal
                } else {
                    accesskit::Orientation::Vertical
                });
                if range.read_only {
                    node.set_read_only();
                } else {
                    node.add_action(Action::SetValue);
                    node.add_action(Action::Increment);
                    node.add_action(Action::Decrement);
                }
            }
            if semantic.editable {
                node.add_action(Action::SetValue);
            }
            if semantic.bounds.intersection(semantic.clip_bounds) != semantic.bounds {
                node.add_action(Action::ScrollIntoView);
            }
            for axis in 0..2 {
                if semantic.scroll_range[axis] > 0. {
                    node.add_action(Action::SetScrollOffset);
                    let offset = f64::from(semantic.scroll_offset[axis]) * f64::from(scale);
                    let max = f64::from(semantic.scroll_range[axis]) * f64::from(scale);
                    if axis == 0 {
                        node.set_scroll_x(offset);
                        node.set_scroll_x_min(0.);
                        node.set_scroll_x_max(max);
                        node.add_action(Action::ScrollLeft);
                        node.add_action(Action::ScrollRight);
                    } else {
                        node.set_scroll_y(offset);
                        node.set_scroll_y_min(0.);
                        node.set_scroll_y_max(max);
                        node.add_action(Action::ScrollUp);
                        node.add_action(Action::ScrollDown);
                    }
                }
            }
            if let Some(value) = semantic.value {
                if semantic.selection.is_some() || semantic.role == SemanticRole::TextInput {
                    let stale = self
                        .runs
                        .get(&semantic.id)
                        .is_none_or(|run| run.revision != semantic.text_revision)
                        && self.value_only_revisions.get(&semantic.id)
                            != Some(&semantic.text_revision);
                    if stale {
                        self.runs.remove(&semantic.id);
                        self.value_only_revisions.remove(&semantic.id);
                        let mut boundaries: Vec<_> =
                            value.grapheme_indices(true).map(|(i, _)| i).collect();
                        if boundaries.last().copied() != Some(value.len()) {
                            boundaries.push(value.len());
                        }
                        if boundaries
                            .windows(2)
                            .all(|pair| pair[1] - pair[0] <= u8::MAX as usize)
                        {
                            let run_id = self.allocate();
                            let mut text = Node::new(Role::TextRun);
                            text.set_value(value);
                            text.set_character_lengths(
                                boundaries
                                    .windows(2)
                                    .map(|p| (p[1] - p[0]) as u8)
                                    .collect::<Vec<_>>(),
                            );
                            self.runs.insert(
                                semantic.id,
                                TextRun {
                                    revision: semantic.text_revision,
                                    id: run_id,
                                    boundaries,
                                    node: Arc::new(text),
                                },
                            );
                            self.stats.prepared_text_runs += 1;
                        } else {
                            self.value_only_revisions
                                .insert(semantic.id, semantic.text_revision);
                        }
                    }
                    if let Some(run) = self.runs.get(&semantic.id) {
                        node.set_children(vec![run.id]);
                        next.insert(run.id, run.node.clone());
                        if !semantic.disabled
                            && let Some(selection) = semantic.selection
                        {
                            let position = |p: TextPosition| {
                                run.boundaries.binary_search(&p.byte_offset).ok().map(
                                    |character_index| accesskit::TextPosition {
                                        node: run.id,
                                        character_index,
                                    },
                                )
                            };
                            if let (Some(anchor), Some(focus)) =
                                (position(selection.anchor), position(selection.focus))
                            {
                                node.set_text_selection(accesskit::TextSelection { anchor, focus });
                                node.add_action(Action::SetTextSelection);
                            }
                        }
                    } else {
                        node.set_value(value);
                    }
                } else {
                    node.set_value(value);
                }
            }
            insert(&self.nodes, &mut next, id, node);
        }
        let focus = ui
            .semantic_focus()
            .filter(|id| visible.contains(id))
            .map_or(root_id, |id| self.ids[&id]);
        let mut changed: Vec<_> = next
            .iter()
            .filter(|(id, node)| {
                full || self
                    .nodes
                    .get(id)
                    .is_none_or(|old| old.as_ref() != node.as_ref())
            })
            .map(|(id, node)| (*id, node.as_ref().clone()))
            .collect();
        changed.sort_by_key(|(id, _)| id.0);
        let publish = full || !changed.is_empty() || focus != self.focus;
        self.reverse = reverse;
        self.nodes = next;
        self.key = Some(key);
        self.title.clear();
        self.title.push_str(title);
        self.scale = scale;
        self.focus = focus;
        if !publish {
            return Ok(None);
        }
        if full {
            self.stats.full_updates += 1;
        } else {
            self.stats.delta_updates += 1;
        }
        let tree = full.then(|| {
            let mut info = TreeInfo::new(root_id);
            info.toolkit_name = Some("RXUI".into());
            info.toolkit_version = Some(env!("CARGO_PKG_VERSION").into());
            info
        });
        Ok(Some(TreeUpdate {
            nodes: changed,
            tree,
            tree_id: TreeId::ROOT,
            focus,
        }))
    }
    /// Decodes a request from the last published tree into a portable action. Text
    /// runs carry their published revision; Ui::semantic_action checks it against
    /// live controlled state. Unknown/subtree IDs and unsupported payloads are ignored.
    pub fn action(&self, request: ActionRequest) -> Option<SemanticAction> {
        if request.target_tree != TreeId::ROOT {
            return None;
        }
        let target = *self.reverse.get(&request.target_node)?;
        let node = self.nodes.get(&request.target_node)?;
        if !node.supports_action(request.action) {
            return None;
        }
        Some(match request.action {
            Action::Focus => SemanticAction::Focus(target),
            Action::Click => SemanticAction::Activate(target),
            Action::ScrollIntoView => SemanticAction::ScrollIntoView(target),
            Action::Increment | Action::Decrement => {
                let value = node.numeric_value()?
                    + node.numeric_value_step().unwrap_or(1.)
                        * if request.action == Action::Increment {
                            1.
                        } else {
                            -1.
                        };
                SemanticAction::SetNumericValue {
                    target,
                    value: value as f32,
                }
            }
            Action::SetValue => match request.data? {
                ActionData::NumericValue(value) => SemanticAction::SetNumericValue {
                    target,
                    value: value as f32,
                },
                ActionData::Value(value) => SemanticAction::SetValue {
                    target,
                    value: value.into(),
                },
                _ => return None,
            },
            Action::SetTextSelection => {
                let ActionData::SetTextSelection(selection) = request.data? else {
                    return None;
                };
                let run = self.runs.get(&target)?;
                let decode = |p: accesskit::TextPosition| {
                    if p.node != run.id {
                        return None;
                    }
                    run.boundaries
                        .get(p.character_index)
                        .copied()
                        .map(TextPosition::new)
                };
                SemanticAction::SetSelection {
                    target,
                    text_revision: run.revision,
                    selection: TextSelection {
                        anchor: decode(selection.anchor)?,
                        focus: decode(selection.focus)?,
                    },
                }
            }
            Action::SetScrollOffset => {
                let ActionData::SetScrollOffset(point) = request.data? else {
                    return None;
                };
                SemanticAction::Scroll {
                    target,
                    offset: [
                        (point.x / f64::from(self.scale)) as f32,
                        (point.y / f64::from(self.scale)) as f32,
                    ],
                }
            }
            Action::ScrollUp | Action::ScrollDown | Action::ScrollLeft | Action::ScrollRight => {
                let axis = usize::from(matches!(
                    request.action,
                    Action::ScrollUp | Action::ScrollDown
                ));
                let negative = matches!(request.action, Action::ScrollUp | Action::ScrollLeft);
                let bounds = node.bounds()?;
                let step = match request.data {
                    Some(ActionData::ScrollUnit(accesskit::ScrollUnit::Item)) => 40.,
                    None | Some(ActionData::ScrollUnit(accesskit::ScrollUnit::Page)) => {
                        (if axis == 0 {
                            bounds.width()
                        } else {
                            bounds.height()
                        }) / f64::from(self.scale)
                    }
                    _ => return None,
                } as f32;
                let mut offset = [
                    node.scroll_x().unwrap_or(0.) as f32 / self.scale,
                    node.scroll_y().unwrap_or(0.) as f32 / self.scale,
                ];
                offset[axis] += if negative { -step } else { step };
                SemanticAction::Scroll { target, offset }
            }
            _ => return None,
        })
    }
}
fn insert(
    old: &HashMap<NodeId, Arc<Node>>,
    next: &mut HashMap<NodeId, Arc<Node>>,
    id: NodeId,
    node: Node,
) {
    let node = old
        .get(&id)
        .filter(|old| old.as_ref() == &node)
        .cloned()
        .unwrap_or_else(|| Arc::new(node));
    next.insert(id, node);
}
fn rect(bounds: crate::Bounds, scale: f32) -> accesskit::Rect {
    let scale = f64::from(scale);
    accesskit::Rect::new(
        f64::from(bounds.x) * scale,
        f64::from(bounds.y) * scale,
        f64::from(bounds.x + bounds.width) * scale,
        f64::from(bounds.y + bounds.height) * scale,
    )
}
fn role(role: SemanticRole) -> Role {
    match role {
        SemanticRole::Dialog => Role::Dialog,
        SemanticRole::Menu => Role::Menu,
        SemanticRole::MenuItem => Role::MenuItem,
        SemanticRole::Tab => Role::Tab,
        SemanticRole::TabList => Role::TabList,
        SemanticRole::TabPanel => Role::TabPanel,
        SemanticRole::Container => Role::GenericContainer,
        SemanticRole::Group => Role::Group,
        SemanticRole::Form => Role::Form,
        SemanticRole::List => Role::List,
        SemanticRole::ListItem => Role::ListItem,
        SemanticRole::Heading => Role::Heading,
        SemanticRole::Label => Role::Label,
        SemanticRole::Image => Role::Image,
        SemanticRole::Button => Role::Button,
        SemanticRole::TextInput => Role::TextInput,
        SemanticRole::Scrollbar => Role::ScrollBar,
        SemanticRole::Splitter => Role::Splitter,
    }
}
