//! Component-native docking workspace model and views.

use astrelis_ui_next::Axis;

use rxui_core::{FrameStyle, View, button, column, label, row, split_pane, views};

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

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: u64) -> DockPane<&'static str> {
        DockPane {
            id,
            title: "Pane".into(),
            value: "value",
        }
    }

    /// Split 1 over pane 10 and split 2 over pane 20 and tab group 3 (21, 22).
    fn layout() -> DockNode<&'static str> {
        DockNode::Split {
            id: 1,
            axis: DockAxis::Horizontal,
            ratio: 0.5,
            first: Box::new(DockNode::Pane(pane(10))),
            second: Box::new(DockNode::Split {
                id: 2,
                axis: DockAxis::Vertical,
                ratio: 0.5,
                first: Box::new(DockNode::Pane(pane(20))),
                second: Box::new(DockNode::Tabs {
                    id: 3,
                    active: 21,
                    panes: vec![pane(21), pane(22)],
                }),
            }),
        }
    }

    fn active_in(node: &DockNode<&'static str>, group: u64) -> Option<u64> {
        match node {
            DockNode::Tabs { id, active, .. } if *id == group => Some(*active),
            DockNode::Split { first, second, .. } => {
                active_in(first, group).or_else(|| active_in(second, group))
            }
            DockNode::Pane(_) | DockNode::Tabs { .. } => None,
        }
    }

    fn ratio_of(node: &DockNode<&'static str>, split: u64) -> Option<f32> {
        match node {
            DockNode::Split {
                id,
                ratio,
                first,
                second,
                ..
            } => {
                if *id == split {
                    Some(*ratio)
                } else {
                    ratio_of(first, split).or_else(|| ratio_of(second, split))
                }
            }
            DockNode::Pane(_) | DockNode::Tabs { .. } => None,
        }
    }

    #[test]
    fn pane_ids_are_reported_in_visual_order() {
        assert_eq!(layout().pane_ids(), vec![10, 20, 21, 22]);
    }

    #[test]
    fn pane_ids_include_every_tab_not_just_the_active_one() {
        let tabs = DockNode::Tabs {
            id: 3,
            active: 21,
            panes: vec![pane(21), pane(22)],
        };
        assert_eq!(tabs.pane_ids(), vec![21, 22]);
    }

    #[test]
    fn pane_ids_of_a_leaf_is_that_leaf() {
        assert_eq!(DockNode::Pane(pane(10)).pane_ids(), vec![10]);
    }

    #[test]
    fn set_ratio_reaches_nested_splits() {
        let mut layout = layout();
        assert!(layout.set_ratio(2, 0.25));
        assert_eq!(ratio_of(&layout, 2), Some(0.25));
        assert_eq!(ratio_of(&layout, 1), Some(0.5));
    }

    #[test]
    fn set_ratio_clamps_into_a_usable_range() {
        let mut layout = layout();
        assert!(layout.set_ratio(1, -3.0));
        assert_eq!(ratio_of(&layout, 1), Some(0.05));
        assert!(layout.set_ratio(1, 12.0));
        assert_eq!(ratio_of(&layout, 1), Some(0.95));
    }

    #[test]
    fn set_ratio_rejects_identities_that_are_not_splits() {
        let mut layout = layout();
        // Tab groups and panes share the identity space with splits, so a
        // ratio addressed at one of them must not silently hit a split.
        assert!(!layout.set_ratio(3, 0.25));
        assert!(!layout.set_ratio(10, 0.25));
        assert!(!layout.set_ratio(999, 0.25));
        assert_eq!(layout, self::layout());
    }

    #[test]
    fn select_activates_a_pane_in_its_own_tab_group() {
        let mut layout = layout();
        assert!(layout.select(22));
        assert_eq!(active_in(&layout, 3), Some(22));
    }

    #[test]
    fn select_ignores_unknown_and_non_tab_panes() {
        let mut layout = layout();
        // Pane 10 is a leaf, not a tab, so there is no activation to change.
        assert!(!layout.select(10));
        assert!(!layout.select(999));
        assert_eq!(layout, self::layout());
    }

    #[test]
    fn select_only_touches_the_group_holding_the_pane() {
        let mut layout = DockNode::Split {
            id: 1,
            axis: DockAxis::Horizontal,
            ratio: 0.5,
            first: Box::new(DockNode::Tabs {
                id: 2,
                active: 20,
                panes: vec![pane(20), pane(21)],
            }),
            second: Box::new(DockNode::Tabs {
                id: 3,
                active: 30,
                panes: vec![pane(30), pane(31)],
            }),
        };
        assert!(layout.select(31));
        assert_eq!(active_in(&layout, 2), Some(20));
        assert_eq!(active_in(&layout, 3), Some(31));
    }
}
