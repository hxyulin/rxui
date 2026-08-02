//! Entity-native docking workspace model and views.

use rxui_core::{Axis, Element, RoutedValueHandler, button, column, label, row, split_pane};

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
        axis: Axis,
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
        if !ratio.is_finite() {
            return false;
        }
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

/// Controlled split-resize proposal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockResize {
    /// Stable split identity.
    pub split: u64,
    /// Proposed first-child fraction.
    pub ratio: f32,
}

/// Builds a controlled docking workspace from core element builders.
pub fn dock_workspace<Pane, RenderPane>(
    layout: &DockNode<Pane>,
    render_pane: RenderPane,
    on_select: RoutedValueHandler<u64>,
    on_resize: RoutedValueHandler<DockResize>,
) -> Element
where
    RenderPane: Fn(&DockPane<Pane>) -> Element + Clone,
{
    match layout {
        DockNode::Pane(pane) => column()
            .children([
                label(pane.title.clone()).key("title"),
                render_pane(pane).key("content"),
            ])
            .key(pane.id),
        DockNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => {
            let first = dock_workspace(
                first,
                render_pane.clone(),
                on_select.clone(),
                on_resize.clone(),
            )
            .key("first");
            let second =
                dock_workspace(second, render_pane, on_select, on_resize.clone()).key("second");
            let split = *id;
            split_pane(*axis, *ratio)
                .children([first, second])
                .on_change(on_resize.map(move |ratio| DockResize { split, ratio }))
                .key(*id)
        }
        DockNode::Tabs { id, active, panes } => {
            let tabs = row().children(panes.iter().map(|pane| {
                button(pane.title.clone())
                    .on_click(on_select.with(pane.id))
                    .key(pane.id)
            }));
            let content = panes
                .iter()
                .find(|pane| pane.id == *active)
                .or_else(|| panes.first())
                .map(render_pane)
                .unwrap_or_else(column);
            column()
                .children([tabs.key("tabs"), content.key("content")])
                .key(*id)
        }
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

    fn layout() -> DockNode<&'static str> {
        DockNode::Split {
            id: 1,
            axis: Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(DockNode::Pane(pane(10))),
            second: Box::new(DockNode::Tabs {
                id: 3,
                active: 21,
                panes: vec![pane(21), pane(22)],
            }),
        }
    }

    #[test]
    fn pane_ids_follow_visual_order() {
        assert_eq!(layout().pane_ids(), vec![10, 21, 22]);
    }

    #[test]
    fn ratio_is_clamped_and_non_finite_is_rejected() {
        let mut layout = layout();
        assert!(layout.set_ratio(1, -3.0));
        let DockNode::Split { ratio, .. } = layout else {
            panic!("expected split")
        };
        assert_eq!(ratio, 0.05);
        let mut layout = self::layout();
        assert!(!layout.set_ratio(1, f32::NAN));
    }

    #[test]
    fn selecting_a_tab_updates_its_group() {
        let mut layout = layout();
        assert!(layout.select(22));
        let DockNode::Split { second, .. } = layout else {
            panic!("expected split")
        };
        let DockNode::Tabs { active, .. } = *second else {
            panic!("expected tabs")
        };
        assert_eq!(active, 22);
    }
}
