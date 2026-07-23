//! Component-native docking workspace model and views.

use astrelis_ui_next::Axis;

use crate::{FrameStyle, View, button, column, label, row, split_pane, views};

/// Dock split direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockAxis {
    /// Children are arranged left-to-right.
    Horizontal,
    /// Children are arranged top-to-bottom.
    Vertical,
}

/// One application-owned dock pane.
#[derive(Clone, Debug, PartialEq)]
pub struct DockPane<Pane> {
    /// Stable pane identity.
    pub id: u64,
    /// User-visible tab title.
    pub title: String,
    /// Application pane model.
    pub value: Pane,
}

/// Persistent dock layout.
#[derive(Clone, Debug, PartialEq)]
pub enum DockNode<Pane> {
    /// One leaf pane.
    Pane(DockPane<Pane>),
    /// Two resizable descendants.
    Split {
        /// Stable split identity.
        id: u64,
        /// Split direction.
        axis: DockAxis,
        /// Fraction assigned to the first descendant.
        ratio: f32,
        /// First descendant.
        first: Box<DockNode<Pane>>,
        /// Second descendant.
        second: Box<DockNode<Pane>>,
    },
    /// Multiple panes sharing one content region.
    Tabs {
        /// Stable tab-group identity.
        id: u64,
        /// Controlled active pane identity.
        active: u64,
        /// Ordered pane collection.
        panes: Vec<DockPane<Pane>>,
    },
}

impl<Pane> DockNode<Pane> {
    /// Returns every pane identity in visual order.
    pub fn pane_ids(&self) -> Vec<u64> {
        let mut output = Vec::new();
        self.collect_pane_ids(&mut output);
        output
    }

    fn collect_pane_ids(&self, output: &mut Vec<u64>) {
        match self {
            Self::Pane(pane) => output.push(pane.id),
            Self::Split { first, second, .. } => {
                first.collect_pane_ids(output);
                second.collect_pane_ids(output);
            }
            Self::Tabs { panes, .. } => output.extend(panes.iter().map(|pane| pane.id)),
        }
    }

    /// Replaces a split ratio by stable identity.
    pub fn set_ratio(&mut self, target: u64, ratio: f32) -> bool {
        match self {
            Self::Split {
                id,
                ratio: current,
                first,
                second,
                ..
            } => {
                if *id == target {
                    *current = ratio.clamp(0.05, 0.95);
                    true
                } else {
                    first.set_ratio(target, ratio) || second.set_ratio(target, ratio)
                }
            }
            Self::Pane(_) | Self::Tabs { .. } => false,
        }
    }

    /// Selects a pane in the tab group containing it.
    pub fn select(&mut self, pane: u64) -> bool {
        match self {
            Self::Tabs { active, panes, .. } if panes.iter().any(|item| item.id == pane) => {
                *active = pane;
                true
            }
            Self::Split { first, second, .. } => first.select(pane) || second.select(pane),
            Self::Pane(_) | Self::Tabs { .. } => false,
        }
    }
}

/// Builds a controlled docking workspace.
pub fn dock_workspace<Action, Pane>(
    layout: &DockNode<Pane>,
    render_pane: impl Fn(&DockPane<Pane>) -> View<Action> + Clone + 'static,
    on_select: impl Fn(u64) -> Action + Clone + 'static,
    on_resize: impl Fn(u64, f32) -> Action + Clone + 'static,
) -> View<Action>
where
    Action: Clone + 'static,
    Pane: 'static,
{
    match layout {
        DockNode::Pane(pane) => column((
            label(pane.title.clone()).key("title"),
            render_pane(pane)
                .frame(FrameStyle::new().grow(1.0))
                .key("content"),
        ))
        .key(pane.id),
        DockNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => {
            let first_view = dock_workspace(
                first,
                render_pane.clone(),
                on_select.clone(),
                on_resize.clone(),
            )
            .key("first");
            let split = *id;
            let resize = on_resize.clone();
            let second_view =
                dock_workspace(second, render_pane, on_select, on_resize).key("second");
            split_pane(
                dock_axis(*axis),
                *ratio,
                first_view,
                second_view,
                move |ratio| resize(split, ratio),
            )
            .key(*id)
        }
        DockNode::Tabs { id, active, panes } => {
            let tabs = row(views(panes.iter().map(|pane| {
                let pane_id = pane.id;
                button(pane.title.clone(), on_select(pane_id)).key(pane_id)
            })));
            let content = panes
                .iter()
                .find(|pane| pane.id == *active)
                .or_else(|| panes.first())
                .map(render_pane)
                .unwrap_or_else(|| column(Vec::new()));
            column((
                tabs.key("tabs"),
                content.frame(FrameStyle::new().grow(1.0)).key("content"),
            ))
            .key(*id)
        }
    }
}

/// Converts dock direction to the retained flex axis.
pub const fn dock_axis(axis: DockAxis) -> Axis {
    match axis {
        DockAxis::Horizontal => Axis::Horizontal,
        DockAxis::Vertical => Axis::Vertical,
    }
}
