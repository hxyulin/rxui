//! Normalized semantic geometry for reviewable layout goldens.

use std::fmt::Write;

use astrelis_core::geometry::LogicalRect;
use rxui_core::core::SemanticNode;

/// One labeled accessible landmark with its resolved window-space geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticLandmark {
    /// Semantic role, rendered as its debug name.
    pub role: String,
    /// Accessible label.
    pub label: String,
    /// Optional accessible value.
    pub value: Option<String>,
    /// Window-space logical bounds.
    pub bounds: LogicalRect,
    /// Effective interaction enablement.
    pub enabled: bool,
    /// Keyboard focus state.
    pub focused: bool,
}

/// A whole semantic snapshot reduced to labeled landmarks in visual order.
///
/// Unlabeled nodes are dropped: they are structural padding and containers that
/// no assistive client can address, and their identities churn with layout
/// refactors that change nothing observable.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SemanticScene {
    /// Labeled semantic landmarks ordered top-to-bottom, then left-to-right.
    pub landmarks: Vec<SemanticLandmark>,
}

impl SemanticScene {
    /// Normalizes a flat semantic snapshot.
    pub fn from_nodes(nodes: &[SemanticNode]) -> Self {
        Self {
            landmarks: nodes
                .iter()
                .filter(|node| !node.data.label.is_empty())
                .map(|node| SemanticLandmark {
                    role: format!("{:?}", node.data.role),
                    label: node.data.label.clone(),
                    value: node.data.value.clone(),
                    bounds: node.bounds,
                    enabled: node.enabled,
                    focused: node.focused,
                })
                .collect(),
        }
        .sorted()
    }

    /// Formats a stable, reviewable semantic geometry snapshot.
    pub fn snapshot(&self) -> String {
        let mut output = String::new();
        for landmark in &self.landmarks {
            let _ = writeln!(
                output,
                "{} label={:?} value={:?} bounds={} enabled={} focused={}",
                landmark.role,
                landmark.label,
                landmark.value,
                rect(landmark.bounds),
                landmark.enabled,
                landmark.focused,
            );
        }
        output
    }

    /// Sorts by position so the snapshot survives arena-allocation reshuffles.
    fn sorted(mut self) -> Self {
        self.landmarks.sort_by(|left, right| {
            left.bounds
                .origin
                .y
                .total_cmp(&right.bounds.origin.y)
                .then_with(|| left.bounds.origin.x.total_cmp(&right.bounds.origin.x))
                .then_with(|| (&left.role, &left.label).cmp(&(&right.role, &right.label)))
        });
        self
    }
}

fn rect(rect: LogicalRect) -> String {
    format!(
        "({},{}) {}x{}",
        number(rect.origin.x),
        number(rect.origin.y),
        number(rect.size.width),
        number(rect.size.height),
    )
}

/// Renders a logical coordinate with signed zero and float dust collapsed.
fn number(value: f32) -> String {
    let value = if value.abs() < 0.0005 { 0.0 } else { value };
    format!("{value:.2}")
}
