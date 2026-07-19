//! Deterministic retained-tree and command testing helpers.

#![warn(missing_docs)]

use astrelis_core::geometry::{LogicalSize, Size};
use std::{collections::HashMap, fmt::Write};

use astrelis_paint::{Command, DisplayList};
use astrelis_text::FontDatabase;
use astrelis_ui_core::{
    ElementInspection, SemanticAction, SemanticNode, SemanticRole, Ui, UiError, UiInspection,
    UiUpdate,
};

/// Deterministic structural snapshots of every public retained representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotBundle {
    /// Accessibility-oriented hierarchy.
    pub semantics: String,
    /// Layout, paint-order, and interaction state.
    pub inspection: String,
    /// Backend-independent paint command stream.
    pub display_list: String,
}

/// Returns a font database containing only Astrelis's bundled Noto Sans face.
pub fn deterministic_font_database() -> FontDatabase {
    astrelis_ui_core::deterministic_font_database()
}

/// Returns the dark Astreon theme pinned to the bundled deterministic font.
pub fn deterministic_theme() -> astrelis_ui_core::Theme {
    astrelis_ui_core::Theme {
        font_families: vec![astrelis_text::FontFamily::Named("Noto Sans".into())],
        ..astrelis_ui_core::Theme::dark()
    }
}

/// Headless harness around one retained UI tree.
pub struct UiHarness<Message = ()> {
    ui: Ui<Message>,
}

impl<Message: 'static> UiHarness<Message> {
    /// Creates a harness with a conventional deterministic viewport.
    pub fn new(ui: Ui<Message>) -> Self {
        Self::with_viewport(ui, Size::new(800.0, 600.0), 1.0)
    }

    /// Creates a harness with an explicit logical viewport and scale factor.
    pub fn with_viewport(mut ui: Ui<Message>, viewport: LogicalSize, scale_factor: f32) -> Self {
        ui.set_viewport(viewport, scale_factor);
        Self { ui }
    }

    /// Returns the retained UI tree.
    pub const fn ui(&self) -> &Ui<Message> {
        &self.ui
    }

    /// Returns the retained UI tree for test setup or updates.
    pub const fn ui_mut(&mut self) -> &mut Ui<Message> {
        &mut self.ui
    }

    /// Produces the current semantic tree after ensuring layout.
    pub fn semantics(&mut self) -> Result<SemanticNode, UiError> {
        self.ui.semantic_tree()
    }

    /// Produces the backend-independent display list.
    pub fn display_list(&mut self) -> Result<DisplayList, UiError> {
        self.ui.display_list()
    }

    /// Produces a normalized semantic-tree snapshot.
    pub fn semantic_snapshot(&mut self) -> Result<String, UiError> {
        Ok(format_semantics(&self.semantics()?))
    }

    /// Produces a normalized retained layout and interaction snapshot.
    pub fn inspection_snapshot(&mut self) -> Result<String, UiError> {
        Ok(format_inspection(&self.ui.inspect()?))
    }

    /// Produces a normalized backend-independent paint-command snapshot.
    pub fn display_list_snapshot(&mut self) -> Result<String, UiError> {
        Ok(format_display_list(&self.display_list()?))
    }

    /// Captures semantics, inspection state, and painting in one operation.
    pub fn snapshot_bundle(&mut self) -> Result<SnapshotBundle, UiError> {
        Ok(SnapshotBundle {
            semantics: self.semantic_snapshot()?,
            inspection: self.inspection_snapshot()?,
            display_list: self.display_list_snapshot()?,
        })
    }

    /// Drains typed application messages emitted by semantic actions or listeners.
    pub fn drain_messages(&mut self) -> impl Iterator<Item = Message> + '_ {
        self.ui.drain_messages()
    }

    /// Finds the first semantic node with the requested role and label.
    pub fn find(
        &mut self,
        role: SemanticRole,
        label: &str,
    ) -> Result<Option<SemanticNode>, UiError> {
        let root = self.semantics()?;
        Ok(find_node(&root, role, label).cloned())
    }

    /// Performs an accessibility action on the first node matching role and label.
    pub fn perform(
        &mut self,
        role: SemanticRole,
        label: &str,
        action: SemanticAction,
    ) -> Result<UiUpdate, UiError> {
        let node = self.find(role, label)?.ok_or_else(|| {
            UiError::from_message(format!("semantic node `{label}` was not found"))
        })?;
        self.ui.perform_semantic_action(node.id, action)
    }

    /// Activates the first semantic node matching role and label.
    pub fn activate(&mut self, role: SemanticRole, label: &str) -> Result<UiUpdate, UiError> {
        self.perform(role, label, SemanticAction::Activate)
    }
}

fn format_semantics(root: &SemanticNode) -> String {
    fn visit(node: &SemanticNode, depth: usize, next_id: &mut usize, output: &mut String) {
        let id = *next_id;
        *next_id += 1;
        let _ = writeln!(
            output,
            "{}#{id} {:?} label={:?} value={:?} bounds={} enabled={} focusable={} focused={} selected={:?} expanded={:?} invalid={} live={:?} actions={:?}",
            "  ".repeat(depth),
            node.role,
            node.label,
            node.value,
            rect(node.bounds),
            node.enabled,
            node.focusable,
            node.focused,
            node.selected,
            node.expanded,
            node.invalid,
            node.live,
            node.actions,
        );
        if let Some(description) = &node.description {
            let _ = writeln!(
                output,
                "{}description={description:?}",
                "  ".repeat(depth + 1)
            );
        }
        if let Some(selection) = node.selection {
            let _ = writeln!(output, "{}selection={selection:?}", "  ".repeat(depth + 1));
        }
        for child in &node.children {
            visit(child, depth + 1, next_id, output);
        }
    }

    let mut output = String::new();
    visit(root, 0, &mut 0, &mut output);
    output
}

fn format_inspection(inspection: &UiInspection) -> String {
    let ids = inspection
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id, index))
        .collect::<HashMap<_, _>>();
    let mut output = format!(
        "viewport={} scale={}\n",
        size(inspection.viewport.width, inspection.viewport.height),
        number(inspection.scale_factor)
    );
    for (index, node) in inspection.nodes.iter().enumerate() {
        let parent = node.parent.and_then(|id| ids.get(&id).copied());
        let _ = writeln!(
            output,
            "#{index} parent={} kind={:?} layout={} world={} physical={} clip={} transform={} z={} rank={} overflow={:?} visible={:?}/{} enabled={} interactive={} hovered={} focused={} focusable={} hit_testable={} declared={}",
            parent.map_or_else(|| "none".into(), |id| format!("#{id}")),
            node.kind,
            rect(node.layout_bounds),
            rect(node.world_bounds),
            rect_physical(node),
            node.clip.map_or_else(|| "none".into(), rect),
            normalize_debug(format!("{:?}", node.world_transform)),
            node.z_index,
            node.paint_rank,
            node.overflow,
            node.visibility,
            node.effectively_visible,
            node.enabled,
            node.interactive,
            node.hovered,
            node.focused,
            node.focusable,
            node.hit_testable,
            normalize_debug(format!("{:?}", node.declared_layout)),
        );
    }
    output
}

fn format_display_list(list: &DisplayList) -> String {
    let mut output = String::new();
    for (index, command) in list.commands().iter().enumerate() {
        match command {
            Command::DrawText {
                text,
                origin,
                opacity,
            } => {
                let layout = list.text(*text);
                let _ = writeln!(
                    output,
                    "{index}: DrawText text={:?} origin={} opacity={} size={}",
                    layout.text(),
                    point(origin.x, origin.y),
                    number(*opacity),
                    size(layout.size().width, layout.size().height),
                );
            }
            Command::FillPath { path, .. }
            | Command::StrokePath { path, .. }
            | Command::ClipPath { path, .. } => {
                let _ = writeln!(
                    output,
                    "{index}: {} path={}",
                    normalize_debug(format!("{command:?}")),
                    normalize_debug(format!("{:?}", list.path(*path).verbs())),
                );
            }
            Command::DrawImage { image, .. } => {
                let resource = list.image(*image);
                let _ = writeln!(
                    output,
                    "{index}: {} resource={:?}",
                    normalize_debug(format!("{command:?}")),
                    resource,
                );
            }
            Command::DrawExternalImage { image, .. } => {
                let resource = list.external_image(*image);
                let _ = writeln!(
                    output,
                    "{index}: {} resource={}x{}",
                    normalize_debug(format!("{command:?}")),
                    resource.size().width,
                    resource.size().height,
                );
            }
            Command::CompositorView {
                destination,
                prefer_direct,
                ..
            } => {
                let _ = writeln!(
                    output,
                    "{index}: CompositorView destination={} prefer_direct={prefer_direct}",
                    rect(*destination),
                );
            }
            _ => {
                let _ = writeln!(
                    output,
                    "{index}: {}",
                    normalize_debug(format!("{command:?}"))
                );
            }
        }
    }
    output
}

fn rect_physical(node: &ElementInspection) -> String {
    format!(
        "({},{} {}x{})",
        number(node.physical_bounds.origin.x),
        number(node.physical_bounds.origin.y),
        number(node.physical_bounds.size.width),
        number(node.physical_bounds.size.height),
    )
}

fn rect(rect: astrelis_core::geometry::LogicalRect) -> String {
    format!(
        "({} {}x{})",
        point(rect.origin.x, rect.origin.y),
        number(rect.size.width),
        number(rect.size.height),
    )
}

fn point(x: f32, y: f32) -> String {
    format!("{},{}", number(x), number(y))
}

fn size(width: f32, height: f32) -> String {
    format!("{}x{}", number(width), number(height))
}

fn number(value: f32) -> String {
    let value = if value == 0.0 { 0.0 } else { value };
    format!("{value:.3}")
}

fn normalize_debug(input: String) -> String {
    let mut output = String::with_capacity(input.len());
    let chars = input.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        let starts_number = chars[index].is_ascii_digit()
            || (chars[index] == '-' && chars.get(index + 1).is_some_and(char::is_ascii_digit));
        if starts_number {
            let start = index;
            index += 1;
            while index < chars.len()
                && (chars[index].is_ascii_digit()
                    || matches!(chars[index], '.' | 'e' | 'E' | '+' | '-'))
            {
                index += 1;
            }
            let token = chars[start..index].iter().collect::<String>();
            if (token.contains('.') || token.contains('e') || token.contains('E'))
                && let Ok(value) = token.parse::<f32>()
            {
                output.push_str(&number(value));
            } else {
                output.push_str(&token);
            }
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    output
}

fn find_node<'a>(
    node: &'a SemanticNode,
    role: SemanticRole,
    label: &str,
) -> Option<&'a SemanticNode> {
    if node.role == role && node.label == label {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_node(child, role, label))
}

#[cfg(test)]
mod tests {
    use astrelis_ui_core::Theme;

    use super::*;

    #[test]
    fn finds_nodes_by_semantic_identity() {
        let mut ui = Ui::<()>::new(FontDatabase::default(), Theme::default());
        ui.add_button(ui.root(), "Save").unwrap();
        let mut harness = UiHarness::new(ui);
        assert!(
            harness
                .find(SemanticRole::Button, "Save")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn activates_controls_by_semantic_identity() {
        let mut ui = Ui::new(FontDatabase::default(), Theme::default());
        let button = ui.add_button(ui.root(), "Save").unwrap();
        ui.listen(
            button,
            None,
            astrelis_ui_core::EventFilter::Activate,
            |context, _| context.emit(7),
        )
        .unwrap();
        let mut harness = UiHarness::new(ui);
        harness.activate(SemanticRole::Button, "Save").unwrap();
        assert_eq!(harness.drain_messages().collect::<Vec<_>>(), vec![7]);
    }

    #[test]
    fn structural_snapshots_are_stable_and_human_readable() {
        fn sample() -> Ui<i32> {
            let mut ui = Ui::new(deterministic_font_database(), deterministic_theme());
            let button = ui.add_button(ui.root(), "Save").unwrap();
            ui.listen(
                button,
                None,
                astrelis_ui_core::EventFilter::Activate,
                |context, _| context.emit(7),
            )
            .unwrap();
            ui
        }

        let first = UiHarness::new(sample()).snapshot_bundle().unwrap();
        let second = UiHarness::new(sample()).snapshot_bundle().unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.semantics,
            include_str!("snapshots/basic.semantics.txt")
        );
        assert_eq!(
            first.inspection,
            include_str!("snapshots/basic.inspection.txt")
        );
        assert_eq!(
            first.display_list,
            include_str!("snapshots/basic.display-list.txt")
        );
        assert!(first.semantics.contains("Button label=\"Save\""));
        assert!(first.inspection.contains("kind=Button"));
        assert!(first.display_list.contains("DrawText text=\"Save\""));
        assert!(!first.inspection.contains("-0.000"));
    }

    #[test]
    fn debug_normalization_rounds_floats_without_changing_integers() {
        assert_eq!(
            normalize_debug("Rect { x: -0.0, y: 1.23456, id: 42 }".into()),
            "Rect { x: 0.000, y: 1.235, id: 42 }"
        );
    }
}
