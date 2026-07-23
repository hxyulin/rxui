//! Normalized semantic geometry for legacy/Next differential tests.

use std::{collections::BTreeMap, fmt::Write};

use astrelis_core::geometry::LogicalRect;
use astrelis_ui_core::SemanticNode as LegacySemanticNode;
use rxui_next::core::SemanticNode as NextSemanticNode;

/// One accessible landmark normalized across both UI implementations.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticLandmark {
    /// Normalized semantic role.
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

impl SemanticLandmark {
    fn key(&self) -> String {
        format!("{}:{:?}", self.role, self.label)
    }
}

/// One normalized semantic scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SemanticScene {
    /// Labeled semantic landmarks in stable visual order.
    pub landmarks: Vec<SemanticLandmark>,
}

impl SemanticScene {
    /// Normalizes a legacy hierarchical semantic tree.
    pub fn from_legacy(root: &LegacySemanticNode) -> Self {
        fn visit(node: &LegacySemanticNode, output: &mut Vec<SemanticLandmark>) {
            if !node.label.is_empty() {
                output.push(SemanticLandmark {
                    role: normalize_role(format!("{:?}", node.role)),
                    label: node.label.clone(),
                    value: node.value.clone(),
                    bounds: node.bounds,
                    enabled: node.enabled,
                    focused: node.focused,
                });
            }
            for child in &node.children {
                visit(child, output);
            }
        }

        let mut landmarks = Vec::new();
        visit(root, &mut landmarks);
        Self { landmarks }.sorted()
    }

    /// Normalizes a Next flat semantic snapshot.
    pub fn from_next(nodes: &[NextSemanticNode]) -> Self {
        Self {
            landmarks: nodes
                .iter()
                .filter(|node| !node.data.label.is_empty())
                .map(|node| SemanticLandmark {
                    role: normalize_role(format!("{:?}", node.data.role)),
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

    fn sorted(mut self) -> Self {
        self.landmarks.sort_by(|left, right| {
            left.bounds
                .origin
                .y
                .total_cmp(&right.bounds.origin.y)
                .then_with(|| left.bounds.origin.x.total_cmp(&right.bounds.origin.x))
                .then_with(|| left.key().cmp(&right.key()))
        });
        self
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
}

/// Side-by-side legacy and Next scene output with geometry deltas.
pub fn differential_snapshot(legacy: &SemanticScene, next: &SemanticScene) -> String {
    let mut output = String::from("[legacy]\n");
    output.push_str(&legacy.snapshot());
    output.push_str("[next]\n");
    output.push_str(&next.snapshot());
    output.push_str("[delta]\n");

    let legacy = indexed(legacy);
    let next = indexed(next);
    for key in legacy.keys().chain(next.keys()) {
        if output.lines().any(|line| line.starts_with(key)) {
            continue;
        }
        match (legacy.get(key), next.get(key)) {
            (Some(legacy), Some(next)) => {
                let _ = writeln!(
                    output,
                    "{key} origin=({},{}) size=({},{})",
                    number(next.bounds.origin.x - legacy.bounds.origin.x),
                    number(next.bounds.origin.y - legacy.bounds.origin.y),
                    number(next.bounds.size.width - legacy.bounds.size.width),
                    number(next.bounds.size.height - legacy.bounds.size.height),
                );
            }
            (Some(_), None) => {
                let _ = writeln!(output, "{key} missing=next");
            }
            (None, Some(_)) => {
                let _ = writeln!(output, "{key} missing=legacy");
            }
            (None, None) => {}
        }
    }
    output
}

/// Verifies that matching landmarks remain within a logical-pixel tolerance.
pub fn compare_geometry(
    legacy: &SemanticScene,
    next: &SemanticScene,
    tolerance: f32,
) -> Result<(), String> {
    let legacy = indexed(legacy);
    let next = indexed(next);
    let mut failures = Vec::new();
    for (key, legacy) in &legacy {
        let Some(next) = next.get(key) else {
            failures.push(format!("{key}: missing from Next"));
            continue;
        };
        for (property, left, right) in [
            ("x", legacy.bounds.origin.x, next.bounds.origin.x),
            ("y", legacy.bounds.origin.y, next.bounds.origin.y),
            ("width", legacy.bounds.size.width, next.bounds.size.width),
            ("height", legacy.bounds.size.height, next.bounds.size.height),
        ] {
            if (left - right).abs() > tolerance {
                failures.push(format!(
                    "{key}: {property} legacy={} next={} tolerance={}",
                    number(left),
                    number(right),
                    number(tolerance)
                ));
            }
        }
    }
    for key in next.keys() {
        if !legacy.contains_key(key) {
            failures.push(format!("{key}: missing from legacy"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn indexed(scene: &SemanticScene) -> BTreeMap<String, &SemanticLandmark> {
    let mut occurrences = BTreeMap::<String, usize>::new();
    let mut output = BTreeMap::new();
    for landmark in &scene.landmarks {
        let base = landmark.key();
        let occurrence = occurrences.entry(base.clone()).or_default();
        let key = format!("{base}[{occurrence}]");
        *occurrence += 1;
        output.insert(key, landmark);
    }
    output
}

fn normalize_role(role: String) -> String {
    match role.as_str() {
        "TableRow" => "Row".into(),
        other => other.into(),
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

fn number(value: f32) -> String {
    let value = if value.abs() < 0.0005 { 0.0 } else { value };
    format!("{value:.2}")
}
