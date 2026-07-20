//! Inspection post-processing shared by the inspector tree and details pane.

use std::collections::{HashMap, HashSet};

use astrelis_core::color::Color;
use astrelis_ui_core::{ElementId, ElementInspection, ElementKind, SemanticNode, SemanticRole};
use astreon_widgets::TreeNode;

/// Cached per-element presentation data consumed by the tree row renderer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowMeta {
    pub kind: ElementKind,
    pub role: Option<SemanticRole>,
    pub label: String,
}

/// Hierarchy rebuilt from one inspection snapshot, creation-ordered.
pub(crate) struct Model {
    pub nodes: HashMap<ElementId, ElementInspection>,
    pub children: HashMap<ElementId, Vec<ElementId>>,
    pub roots: Vec<ElementId>,
}

impl Model {
    pub fn build(nodes: Vec<ElementInspection>) -> Self {
        let ids = nodes.iter().map(|node| node.id).collect::<HashSet<_>>();
        let mut children: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
        let mut roots = Vec::new();
        for node in &nodes {
            match node.parent.filter(|parent| ids.contains(parent)) {
                Some(parent) => children.entry(parent).or_default().push(node.id),
                None => roots.push(node.id),
            }
        }
        Self {
            nodes: nodes.into_iter().map(|node| (node.id, node)).collect(),
            children,
            roots,
        }
    }

    /// Root-first inclusive ancestor chain for one element.
    pub fn ancestor_chain(&self, id: ElementId) -> Vec<ElementId> {
        let mut chain = vec![id];
        let mut current = id;
        while let Some(parent) = self.nodes.get(&current).and_then(|node| node.parent) {
            if !self.nodes.contains_key(&parent) {
                break;
            }
            chain.push(parent);
            current = parent;
        }
        chain.reverse();
        chain
    }

    /// Ids of every element at most `depth` levels below a root.
    pub fn ids_up_to_depth(&self, depth: usize) -> HashSet<ElementId> {
        let mut output = HashSet::new();
        let mut frontier = self.roots.clone();
        for _ in 0..=depth {
            let mut next = Vec::new();
            for id in frontier {
                output.insert(id);
                next.extend(self.children.get(&id).into_iter().flatten().copied());
            }
            frontier = next;
        }
        output
    }
}

/// Extracts the row metadata for every element in the model.
pub(crate) fn row_meta(
    model: &Model,
    semantics: &HashMap<ElementId, (SemanticRole, String)>,
) -> HashMap<ElementId, RowMeta> {
    model
        .nodes
        .values()
        .map(|node| {
            let semantic = semantics.get(&node.id);
            (
                node.id,
                RowMeta {
                    kind: node.kind,
                    role: semantic.map(|(role, _)| *role),
                    label: semantic
                        .map(|(_, label)| label.clone())
                        .unwrap_or_default(),
                },
            )
        })
        .collect()
}

/// Flattens the semantic tree into id-indexed role/label pairs.
pub(crate) fn semantic_labels(root: &SemanticNode) -> HashMap<ElementId, (SemanticRole, String)> {
    fn visit(node: &SemanticNode, output: &mut HashMap<ElementId, (SemanticRole, String)>) {
        output.insert(node.id, (node.role, node.label.clone()));
        for child in &node.children {
            visit(child, output);
        }
    }
    let mut output = HashMap::new();
    visit(root, &mut output);
    output
}

/// Whether one element matches a case-insensitive tree filter.
fn matches(meta: &RowMeta, filter: &str) -> bool {
    format!("{:?}", meta.kind).to_lowercase().contains(filter)
        || meta
            .role
            .is_some_and(|role| format!("{role:?}").to_lowercase().contains(filter))
        || meta.label.to_lowercase().contains(filter)
}

/// Builds the controlled [`TreeNode`] hierarchy honoring expansion and filter.
///
/// A non-empty filter keeps matching elements plus their ancestors and forces
/// the kept branches open without persisting that expansion.
pub(crate) fn tree_nodes(
    model: &Model,
    meta: &HashMap<ElementId, RowMeta>,
    expanded: &HashSet<ElementId>,
    filter: &str,
) -> Vec<TreeNode<ElementId>> {
    let filter = filter.trim().to_lowercase();
    let keep = if filter.is_empty() {
        None
    } else {
        let mut keep = HashSet::new();
        for (id, row) in meta {
            if matches(row, &filter) {
                keep.extend(model.ancestor_chain(*id));
            }
        }
        Some(keep)
    };

    fn build(
        model: &Model,
        meta: &HashMap<ElementId, RowMeta>,
        expanded: &HashSet<ElementId>,
        keep: Option<&HashSet<ElementId>>,
        id: ElementId,
    ) -> Option<TreeNode<ElementId>> {
        if keep.is_some_and(|keep| !keep.contains(&id)) {
            return None;
        }
        let children = model
            .children
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|child| build(model, meta, expanded, keep, *child))
            .collect::<Vec<_>>();
        let row = meta.get(&id);
        let label = row.map_or_else(
            || "Element".to_string(),
            |row| {
                let mut label = format!("{:?}", row.kind);
                if let Some(role) = row.role {
                    label.push_str(&format!(" {role:?}"));
                }
                if !row.label.is_empty() {
                    label.push_str(&format!(" \"{}\"", row.label));
                }
                label
            },
        );
        let node = TreeNode::leaf(id, label)
            .expanded(keep.is_some() || expanded.contains(&id))
            .children(children);
        Some(node)
    }

    model
        .roots
        .iter()
        .filter_map(|root| build(model, meta, expanded, keep.as_ref(), *root))
        .collect()
}

/// Devtools syntax color for one element kind, chosen for dark surfaces.
pub(crate) fn kind_color(kind: ElementKind) -> Color {
    match kind {
        ElementKind::Row
        | ElementKind::Column
        | ElementKind::Stack
        | ElementKind::Padding
        | ElementKind::ScrollView
        | ElementKind::Overlay
        | ElementKind::FocusScope => Color::from_hex(0x7fb1ff),
        ElementKind::Button
        | ElementKind::TextField
        | ElementKind::Checkbox
        | ElementKind::Slider => Color::from_hex(0xc390e4),
        ElementKind::Custom => Color::from_hex(0x5fc7ba),
        _ => Color::from_hex(0xc9c9d1),
    }
}

/// Devtools accent for quoted semantic labels.
pub(crate) fn label_color() -> Color {
    Color::from_hex(0xd8a457)
}
