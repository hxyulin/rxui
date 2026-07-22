use std::{collections::HashSet, error::Error, fmt};

use astrelis_core::geometry::{LogicalSize, Size};
use serde::{Deserialize, Serialize};

/// Error produced by a docking model or workspace operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockError(String);

impl DockError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for DockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Error for DockError {}

/// Stable, application-defined identity of one dockable panel.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PanelId(String);

impl PanelId {
    /// Creates a non-empty panel identity.
    pub fn new(value: impl Into<String>) -> Result<Self, DockError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DockError::new("panel identity must not be empty"));
        }
        Ok(Self(value))
    }

    /// Returns the persisted string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PanelId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Axis along which a dock node divides its children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockAxis {
    /// Children are arranged left and right.
    Horizontal,
    /// Children are arranged top and bottom.
    Vertical,
}

/// Edge at which a panel creates a new split.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockSide {
    /// Insert to the left of the target.
    Left,
    /// Insert to the right of the target.
    Right,
    /// Insert above the target.
    Top,
    /// Insert below the target.
    Bottom,
}

impl DockSide {
    pub(crate) const fn axis(self) -> DockAxis {
        match self {
            Self::Left | Self::Right => DockAxis::Horizontal,
            Self::Top | Self::Bottom => DockAxis::Vertical,
        }
    }

    fn inserted_first(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

/// Serializable logical-pixel rectangle for an in-window floating group.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FloatingRect {
    /// Horizontal position relative to the workspace.
    pub x: f32,
    /// Vertical position relative to the workspace.
    pub y: f32,
    /// Logical width.
    pub width: f32,
    /// Logical height.
    pub height: f32,
}

impl FloatingRect {
    /// Creates floating geometry.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub(crate) fn repaired(self, fallback: Self) -> Self {
        if [self.x, self.y, self.width, self.height]
            .into_iter()
            .all(f32::is_finite)
            && self.width > 0.0
            && self.height > 0.0
        {
            self
        } else {
            fallback
        }
    }

    /// Clamps size to panel minima and keeps the title chrome reachable.
    pub fn clamp_to_viewport(
        self,
        viewport: LogicalSize,
        minimum: LogicalSize,
        reachable_title: f32,
    ) -> Self {
        let width = self.width.max(minimum.width.max(1.0));
        let height = self.height.max(minimum.height.max(1.0));
        let reachable = reachable_title.max(1.0).min(width);
        let x = self
            .x
            .clamp(reachable - width, (viewport.width - reachable).max(0.0));
        let title_height = reachable_title.max(1.0).min(height);
        let y = self.y.clamp(0.0, (viewport.height - title_height).max(0.0));
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// One ordered group of tabs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockTabs {
    /// Panel identities in visual tab order.
    pub panels: Vec<PanelId>,
    /// Currently visible panel.
    pub active: PanelId,
}

impl DockTabs {
    /// Creates a non-empty tab group with its first panel active.
    pub fn new(panels: Vec<PanelId>) -> Result<Self, DockError> {
        let Some(active) = panels.first().cloned() else {
            return Err(DockError::new("tab groups must contain a panel"));
        };
        Ok(Self { panels, active })
    }
}

/// Serializable node in the main docking tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DockNode {
    /// An ordered group of panels sharing one content area.
    Tabs(DockTabs),
    /// Two dock nodes separated by a resizable divider.
    Split {
        /// Split orientation.
        axis: DockAxis,
        /// Fraction assigned to the first child.
        ratio: f32,
        /// First child.
        first: Box<DockNode>,
        /// Second child.
        second: Box<DockNode>,
    },
}

/// One movable and resizable in-window floating tab group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FloatingGroup {
    /// Tabs hosted by the floating surface.
    pub tabs: DockTabs,
    /// Logical geometry relative to the workspace.
    pub bounds: FloatingRect,
}

/// Serializable docking state, excluding all application widget state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DockLayout {
    /// Main dock tree, or `None` for an empty main area.
    pub root: Option<DockNode>,
    /// Floating groups ordered from back to front.
    pub floating: Vec<FloatingGroup>,
}

/// Preferred location used when a panel is opened or recovered.
#[derive(Clone, Debug, PartialEq)]
pub enum PreferredPlacement {
    /// Append to the main root's first tab group.
    Root,
    /// Join the group containing the anchor panel.
    Tab(PanelId),
    /// Split relative to the group containing the anchor panel.
    Split {
        /// Panel identifying the target group.
        anchor: PanelId,
        /// Edge at which the panel is inserted.
        side: DockSide,
    },
    /// Open as a floating panel at the supplied geometry.
    Floating(FloatingRect),
}

/// Application-owned metadata for one registered panel.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelDescriptor {
    /// Stable identity.
    pub id: PanelId,
    /// Human-readable tab and semantic label.
    pub title: String,
    /// Minimum logical content size.
    pub minimum_size: LogicalSize,
    /// Whether users may remove the panel from the visible layout.
    pub closable: bool,
    /// Deterministic location used when opening or recovering the panel.
    pub preferred: PreferredPlacement,
}

impl PanelDescriptor {
    /// Creates a descriptor with a root-tab preference and an 80×80 minimum.
    pub fn new(id: PanelId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            minimum_size: Size::new(80.0, 80.0),
            closable: true,
            preferred: PreferredPlacement::Root,
        }
    }

    /// Sets whether users may remove the panel, returning the descriptor so
    /// registration reads as one chain instead of create-then-poke.
    #[must_use]
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    /// Sets the minimum logical content size.
    #[must_use]
    pub fn minimum_size(mut self, minimum_size: LogicalSize) -> Self {
        self.minimum_size = minimum_size;
        self
    }

    /// Sets the deterministic placement used when opening or recovering it.
    #[must_use]
    pub fn preferred(mut self, preferred: PreferredPlacement) -> Self {
        self.preferred = preferred;
        self
    }
}

/// Concrete destination of a panel movement.
#[derive(Clone, Debug, PartialEq)]
pub enum DockPlacement {
    /// Insert into the main root's first tab group.
    Root {
        /// Requested insertion index in the first root tab group.
        index: usize,
    },
    /// Insert into the anchor panel's group.
    Tab {
        /// Panel identifying the target group.
        anchor: PanelId,
        /// Requested tab insertion index.
        index: usize,
    },
    /// Create a split around the anchor panel's group.
    Split {
        /// Panel identifying the target group.
        anchor: PanelId,
        /// Edge at which the new group is inserted.
        side: DockSide,
    },
    /// Create a new floating group.
    Floating(FloatingRect),
}

/// Repairs made while reconciling persisted state with registered panels.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NormalizationReport {
    /// Number of unknown or invalid identities removed.
    pub removed_unknown: usize,
    /// Number of duplicate occurrences removed.
    pub removed_duplicates: usize,
    /// Whether the supplied default layout was used.
    pub used_default: bool,
    /// Required panels reinserted during recovery.
    pub inserted_required: Vec<PanelId>,
}

impl DockLayout {
    /// Returns whether the layout contains a panel.
    pub fn contains(&self, panel: &PanelId) -> bool {
        self.root
            .as_ref()
            .is_some_and(|root| node_contains(root, panel))
            || self
                .floating
                .iter()
                .any(|group| group.tabs.panels.contains(panel))
    }

    /// Returns panel identities in deterministic model order.
    pub fn panels(&self) -> Vec<&PanelId> {
        let mut panels = Vec::new();
        if let Some(root) = &self.root {
            collect_panels(root, &mut panels);
        }
        for floating in &self.floating {
            panels.extend(floating.tabs.panels.iter());
        }
        panels
    }

    /// Activates a visible panel and raises its floating group when applicable.
    pub fn activate(&mut self, panel: &PanelId) -> Result<(), DockError> {
        if self
            .root
            .as_mut()
            .is_some_and(|root| activate_in_node(root, panel))
        {
            return Ok(());
        }
        let Some(index) = self
            .floating
            .iter()
            .position(|group| group.tabs.panels.contains(panel))
        else {
            return Err(DockError::new(format!("panel {panel} is not visible")));
        };
        self.floating[index].tabs.active = panel.clone();
        let group = self.floating.remove(index);
        self.floating.push(group);
        Ok(())
    }

    /// Removes a visible panel and collapses empty branches.
    pub fn remove_panel(&mut self, panel: &PanelId) -> bool {
        let mut removed = false;
        self.root = self.root.take().and_then(|root| {
            let (root, did_remove) = remove_from_node(root, panel);
            removed |= did_remove;
            root
        });
        if !removed
            && let Some(group_index) = self
                .floating
                .iter()
                .position(|group| group.tabs.panels.contains(panel))
        {
            removed = remove_from_tabs(&mut self.floating[group_index].tabs, panel);
            if self.floating[group_index].tabs.panels.is_empty() {
                self.floating.remove(group_index);
            }
        }
        removed
    }

    /// Moves an existing panel, or inserts a currently absent registered panel.
    pub fn place_panel(
        &mut self,
        panel: PanelId,
        placement: DockPlacement,
    ) -> Result<(), DockError> {
        let original = self.clone();
        self.remove_panel(&panel);
        let result = match placement {
            DockPlacement::Root { index } => {
                if let Some(tabs) = self.root.as_mut().and_then(first_tabs_mut) {
                    insert_tab(tabs, panel, index);
                } else {
                    self.root = Some(DockNode::Tabs(DockTabs {
                        active: panel.clone(),
                        panels: vec![panel],
                    }));
                }
                Ok(())
            }
            DockPlacement::Tab { anchor, index } => find_tabs_mut(self, &anchor)
                .ok_or_else(|| DockError::new(format!("anchor panel {anchor} is not visible")))
                .map(|tabs| insert_tab(tabs, panel, index)),
            DockPlacement::Split { anchor, side } => {
                if let Some(root) = self.root.as_mut() {
                    let inserted = DockNode::Tabs(DockTabs {
                        active: panel.clone(),
                        panels: vec![panel],
                    });
                    if split_at_anchor(root, &anchor, side, inserted) {
                        Ok(())
                    } else {
                        Err(DockError::new(format!(
                            "split anchor panel {anchor} is not in the main dock tree"
                        )))
                    }
                } else {
                    Err(DockError::new("cannot split an empty main dock tree"))
                }
            }
            DockPlacement::Floating(bounds) => {
                self.floating.push(FloatingGroup {
                    tabs: DockTabs {
                        active: panel.clone(),
                        panels: vec![panel],
                    },
                    bounds,
                });
                Ok(())
            }
        };
        if result.is_err() {
            *self = original;
        }
        result
    }

    /// Repairs persisted state against the registered panel set and fallback layout.
    pub fn normalize(
        &mut self,
        descriptors: &[PanelDescriptor],
        default: &DockLayout,
        default_float: FloatingRect,
    ) -> NormalizationReport {
        let known = descriptors
            .iter()
            .map(|descriptor| descriptor.id.clone())
            .collect::<HashSet<_>>();
        let mut report = NormalizationReport::default();
        normalize_once(self, &known, default_float, &mut report);
        if self.panels().is_empty() && !default.panels().is_empty() {
            *self = default.clone();
            report.used_default = true;
            normalize_once(self, &known, default_float, &mut report);
        }
        for descriptor in descriptors.iter().filter(|descriptor| !descriptor.closable) {
            if !self.contains(&descriptor.id) {
                let placement = resolve_preferred(self, &descriptor.preferred, default_float);
                let _ = self.place_panel(descriptor.id.clone(), placement);
                report.inserted_required.push(descriptor.id.clone());
            }
        }
        report
    }
}

fn normalize_once(
    layout: &mut DockLayout,
    known: &HashSet<PanelId>,
    default_float: FloatingRect,
    report: &mut NormalizationReport,
) {
    let mut seen = HashSet::new();
    layout.root = layout
        .root
        .take()
        .and_then(|root| normalize_node(root, known, &mut seen, report));
    layout.floating = layout
        .floating
        .drain(..)
        .filter_map(|mut group| {
            normalize_tabs(&mut group.tabs, known, &mut seen, report);
            if group.tabs.panels.is_empty() {
                None
            } else {
                group.bounds = group.bounds.repaired(default_float);
                Some(group)
            }
        })
        .collect();
}

fn normalize_node(
    node: DockNode,
    known: &HashSet<PanelId>,
    seen: &mut HashSet<PanelId>,
    report: &mut NormalizationReport,
) -> Option<DockNode> {
    match node {
        DockNode::Tabs(mut tabs) => {
            normalize_tabs(&mut tabs, known, seen, report);
            (!tabs.panels.is_empty()).then_some(DockNode::Tabs(tabs))
        }
        DockNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let first = normalize_node(*first, known, seen, report);
            let second = normalize_node(*second, known, seen, report);
            match (first, second) {
                (Some(first), Some(second)) => Some(DockNode::Split {
                    axis,
                    ratio: if ratio.is_finite() {
                        ratio.clamp(0.0, 1.0)
                    } else {
                        0.5
                    },
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(child), None) | (None, Some(child)) => Some(child),
                (None, None) => None,
            }
        }
    }
}

fn normalize_tabs(
    tabs: &mut DockTabs,
    known: &HashSet<PanelId>,
    seen: &mut HashSet<PanelId>,
    report: &mut NormalizationReport,
) {
    tabs.panels.retain(|panel| {
        if panel.as_str().trim().is_empty() || !known.contains(panel) {
            report.removed_unknown += 1;
            false
        } else if !seen.insert(panel.clone()) {
            report.removed_duplicates += 1;
            false
        } else {
            true
        }
    });
    if !tabs.panels.contains(&tabs.active)
        && let Some(first) = tabs.panels.first()
    {
        tabs.active = first.clone();
    }
}

fn resolve_preferred(
    layout: &DockLayout,
    preferred: &PreferredPlacement,
    _default_float: FloatingRect,
) -> DockPlacement {
    match preferred {
        PreferredPlacement::Root => DockPlacement::Root { index: usize::MAX },
        PreferredPlacement::Tab(anchor) if layout.contains(anchor) => DockPlacement::Tab {
            anchor: anchor.clone(),
            index: usize::MAX,
        },
        PreferredPlacement::Split { anchor, side }
            if layout
                .root
                .as_ref()
                .is_some_and(|root| node_contains(root, anchor)) =>
        {
            DockPlacement::Split {
                anchor: anchor.clone(),
                side: *side,
            }
        }
        PreferredPlacement::Floating(bounds) => DockPlacement::Floating(*bounds),
        _ => DockPlacement::Root { index: usize::MAX },
    }
}

fn node_contains(node: &DockNode, panel: &PanelId) -> bool {
    match node {
        DockNode::Tabs(tabs) => tabs.panels.contains(panel),
        DockNode::Split { first, second, .. } => {
            node_contains(first, panel) || node_contains(second, panel)
        }
    }
}

fn collect_panels<'a>(node: &'a DockNode, panels: &mut Vec<&'a PanelId>) {
    match node {
        DockNode::Tabs(tabs) => panels.extend(tabs.panels.iter()),
        DockNode::Split { first, second, .. } => {
            collect_panels(first, panels);
            collect_panels(second, panels);
        }
    }
}

fn activate_in_node(node: &mut DockNode, panel: &PanelId) -> bool {
    match node {
        DockNode::Tabs(tabs) if tabs.panels.contains(panel) => {
            tabs.active = panel.clone();
            true
        }
        DockNode::Tabs(_) => false,
        DockNode::Split { first, second, .. } => {
            activate_in_node(first, panel) || activate_in_node(second, panel)
        }
    }
}

fn remove_from_tabs(tabs: &mut DockTabs, panel: &PanelId) -> bool {
    let Some(index) = tabs.panels.iter().position(|candidate| candidate == panel) else {
        return false;
    };
    tabs.panels.remove(index);
    if tabs.active == *panel
        && let Some(next) = tabs
            .panels
            .get(index.min(tabs.panels.len().saturating_sub(1)))
    {
        tabs.active = next.clone();
    }
    true
}

fn remove_from_node(node: DockNode, panel: &PanelId) -> (Option<DockNode>, bool) {
    match node {
        DockNode::Tabs(mut tabs) => {
            let removed = remove_from_tabs(&mut tabs, panel);
            (
                (!tabs.panels.is_empty()).then_some(DockNode::Tabs(tabs)),
                removed,
            )
        }
        DockNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let (first, removed_first) = remove_from_node(*first, panel);
            let (second, removed_second) = if removed_first {
                (Some(*second), false)
            } else {
                remove_from_node(*second, panel)
            };
            let node = match (first, second) {
                (Some(first), Some(second)) => Some(DockNode::Split {
                    axis,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(child), None) | (None, Some(child)) => Some(child),
                (None, None) => None,
            };
            (node, removed_first || removed_second)
        }
    }
}

fn first_tabs_mut(node: &mut DockNode) -> Option<&mut DockTabs> {
    match node {
        DockNode::Tabs(tabs) => Some(tabs),
        DockNode::Split { first, .. } => first_tabs_mut(first),
    }
}

fn find_tabs_mut<'a>(layout: &'a mut DockLayout, anchor: &PanelId) -> Option<&'a mut DockTabs> {
    if let Some(tabs) = layout
        .root
        .as_mut()
        .and_then(|root| find_tabs_in_node(root, anchor))
    {
        return Some(tabs);
    }
    layout
        .floating
        .iter_mut()
        .find(|group| group.tabs.panels.contains(anchor))
        .map(|group| &mut group.tabs)
}

fn find_tabs_in_node<'a>(node: &'a mut DockNode, anchor: &PanelId) -> Option<&'a mut DockTabs> {
    match node {
        DockNode::Tabs(tabs) => tabs.panels.contains(anchor).then_some(tabs),
        DockNode::Split { first, second, .. } => {
            find_tabs_in_node(first, anchor).or_else(|| find_tabs_in_node(second, anchor))
        }
    }
}

fn insert_tab(tabs: &mut DockTabs, panel: PanelId, index: usize) {
    let index = index.min(tabs.panels.len());
    tabs.panels.insert(index, panel.clone());
    tabs.active = panel;
}

fn split_at_anchor(
    node: &mut DockNode,
    anchor: &PanelId,
    side: DockSide,
    inserted: DockNode,
) -> bool {
    if matches!(node, DockNode::Tabs(tabs) if tabs.panels.contains(anchor)) {
        let replaced = std::mem::replace(
            node,
            DockNode::Tabs(DockTabs {
                panels: Vec::new(),
                active: anchor.clone(),
            }),
        );
        let (first, second) = if side.inserted_first() {
            (inserted, replaced)
        } else {
            (replaced, inserted)
        };
        *node = DockNode::Split {
            axis: side.axis(),
            ratio: 0.5,
            first: Box::new(first),
            second: Box::new(second),
        };
        return true;
    }
    match node {
        DockNode::Split { first, second, .. } => {
            split_at_anchor(first, anchor, side, inserted.clone())
                || split_at_anchor(second, anchor, side, inserted)
        }
        DockNode::Tabs(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> PanelId {
        PanelId::new(value).unwrap()
    }

    fn descriptor(value: &str, closable: bool) -> PanelDescriptor {
        PanelDescriptor {
            closable,
            ..PanelDescriptor::new(id(value), value)
        }
    }

    #[test]
    fn serde_round_trip_contains_only_layout_state() {
        let layout = DockLayout {
            root: Some(DockNode::Split {
                axis: DockAxis::Horizontal,
                ratio: 0.25,
                first: Box::new(DockNode::Tabs(DockTabs::new(vec![id("scene")]).unwrap())),
                second: Box::new(DockNode::Tabs(
                    DockTabs::new(vec![id("inspector"), id("assets")]).unwrap(),
                )),
            }),
            floating: vec![FloatingGroup {
                tabs: DockTabs::new(vec![id("console")]).unwrap(),
                bounds: FloatingRect::new(20.0, 30.0, 400.0, 240.0),
            }],
        };
        let json = serde_json::to_string(&layout).unwrap();
        assert_eq!(serde_json::from_str::<DockLayout>(&json).unwrap(), layout);
        assert!(!json.contains("widget"));
    }

    #[test]
    fn moving_and_closing_panels_collapse_tree() {
        let mut layout = DockLayout {
            root: Some(DockNode::Tabs(
                DockTabs::new(vec![id("a"), id("b")]).unwrap(),
            )),
            floating: Vec::new(),
        };
        layout
            .place_panel(
                id("b"),
                DockPlacement::Split {
                    anchor: id("a"),
                    side: DockSide::Right,
                },
            )
            .unwrap();
        assert!(matches!(layout.root, Some(DockNode::Split { .. })));
        assert!(layout.remove_panel(&id("a")));
        assert!(matches!(layout.root, Some(DockNode::Tabs(_))));
        assert_eq!(layout.panels(), vec![&id("b")]);
    }

    #[test]
    fn normalization_prunes_duplicates_repairs_and_recovers_required() {
        let mut layout = DockLayout {
            root: Some(DockNode::Split {
                axis: DockAxis::Vertical,
                ratio: f32::NAN,
                first: Box::new(DockNode::Tabs(DockTabs {
                    panels: vec![id("known"), id("missing")],
                    active: id("missing"),
                })),
                second: Box::new(DockNode::Tabs(DockTabs {
                    panels: vec![id("known")],
                    active: id("known"),
                })),
            }),
            floating: Vec::new(),
        };
        let descriptors = vec![descriptor("known", true), descriptor("required", false)];
        let report = layout.normalize(
            &descriptors,
            &DockLayout::default(),
            FloatingRect::new(10.0, 10.0, 320.0, 240.0),
        );
        assert_eq!(report.removed_unknown, 1);
        assert_eq!(report.removed_duplicates, 1);
        assert_eq!(report.inserted_required, vec![id("required")]);
        assert_eq!(layout.panels(), vec![&id("known"), &id("required")]);
    }

    #[test]
    fn unusable_saved_layout_falls_back_to_default() {
        let mut saved = DockLayout {
            root: Some(DockNode::Tabs(DockTabs::new(vec![id("stale")]).unwrap())),
            floating: Vec::new(),
        };
        let default = DockLayout {
            root: Some(DockNode::Tabs(DockTabs::new(vec![id("known")]).unwrap())),
            floating: Vec::new(),
        };
        let report = saved.normalize(
            &[descriptor("known", true)],
            &default,
            FloatingRect::new(0.0, 0.0, 320.0, 240.0),
        );
        assert!(report.used_default);
        assert!(saved.contains(&id("known")));
    }

    #[test]
    fn invalid_move_is_atomic() {
        let original = DockLayout {
            root: Some(DockNode::Tabs(
                DockTabs::new(vec![id("a"), id("b")]).unwrap(),
            )),
            floating: Vec::new(),
        };
        let mut layout = original.clone();
        assert!(
            layout
                .place_panel(
                    id("a"),
                    DockPlacement::Tab {
                        anchor: id("missing"),
                        index: 0,
                    },
                )
                .is_err()
        );
        assert_eq!(layout, original);
    }
}
