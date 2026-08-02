//! Reviewable semantic geometry for deterministic hand-built tree scenes.

use std::fmt::Write;

use astrelis_core::{color::Color, geometry::LogicalSize};
use astrelis_text::FontDatabase;
use rxui_test::{SemanticScene, assert_text_golden};
use rxui_tree::{Axis, BoxElement, Flex, Frame, Label, SemanticData, SemanticRole, Stack, UiTree};

const GOLDEN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/goldens/synthetic-scenes.txt"
);

fn landmark(label: &str) -> BoxElement {
    BoxElement {
        size: LogicalSize::ZERO,
        color: Color::WHITE,
        semantics: Some(SemanticData {
            role: SemanticRole::Group,
            label: label.into(),
            ..SemanticData::default()
        }),
        interactive: false,
    }
}

fn framed_landmark(ui: &mut UiTree, parent: rxui_tree::NodeId, label: &str, grow: f32) {
    let frame = ui.append(
        parent,
        Frame {
            grow,
            ..Frame::default()
        },
    );
    ui.append(frame.id(), landmark(label));
}

fn finish_scene(output: &mut String, section: &str, ui: &mut UiTree) -> SemanticScene {
    ui.update_passes();
    let scene = SemanticScene::from_nodes(&ui.semantic_snapshot());
    let _ = writeln!(output, "[{section}]");
    output.push_str(&scene.snapshot());
    scene
}

fn ratio_split(output: &mut String) {
    let mut ui = UiTree::with_fonts(
        Flex {
            axis: Axis::Horizontal,
            gap: 6.0,
            ..Flex::default()
        },
        LogicalSize::new(400.0, 240.0),
        FontDatabase::empty(),
    );
    let root = ui.root();
    framed_landmark(&mut ui, root, "First pane", 1.0);
    framed_landmark(&mut ui, root, "Second pane", 3.0);
    finish_scene(output, "ratio split 1-3", &mut ui);
}

fn grow_row(output: &mut String) {
    let mut ui = UiTree::with_fonts(
        Flex {
            axis: Axis::Horizontal,
            ..Flex::default()
        },
        LogicalSize::new(400.0, 120.0),
        FontDatabase::empty(),
    );
    let root = ui.root();
    framed_landmark(&mut ui, root, "Grow 1a", 1.0);
    framed_landmark(&mut ui, root, "Grow 2", 2.0);
    framed_landmark(&mut ui, root, "Grow 1b", 1.0);
    finish_scene(output, "grow 1-2-1", &mut ui);
}

fn overlapping_stack(output: &mut String) {
    let mut ui = UiTree::with_fonts(
        Stack::default(),
        LogicalSize::new(160.0, 80.0),
        FontDatabase::empty(),
    );
    let root = ui.root();
    for label in ["Back layer", "Front layer"] {
        let frame = ui.append(
            root,
            Frame {
                width: Some(120.0),
                height: Some(40.0),
                ..Frame::default()
            },
        );
        ui.append(frame.id(), landmark(label));
    }
    finish_scene(output, "stack overlap", &mut ui);
}

fn labeled_form(output: &mut String) {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Vertical,
            gap: 8.0,
            padding: 12.0,
            ..Flex::default()
        },
        LogicalSize::new(260.0, 140.0),
    );
    let root = ui.root();
    let heading = ui.append(
        root,
        Frame {
            width: Some(236.0),
            height: Some(20.0),
            ..Frame::default()
        },
    );
    ui.append(heading.id(), Label::new("Account").with_font_size(16.0));
    for label in ["Name", "Email"] {
        let frame = ui.append(
            root,
            Frame {
                width: Some(236.0),
                height: Some(28.0),
                ..Frame::default()
            },
        );
        let mut field = landmark(label);
        field.semantics.as_mut().expect("semantics").role = SemanticRole::Field;
        ui.append(frame.id(), field);
    }
    let scene = finish_scene(output, "labeled form", &mut ui);
    let stats = ui.stats();
    assert_eq!(stats.shaped_text, 1, "the Label must pass through shaping");
    let account = scene
        .landmarks
        .iter()
        .find(|landmark| landmark.label == "Account")
        .expect("Account landmark");
    assert_eq!(
        account.bounds.size,
        LogicalSize::new(236.0, 20.0),
        "the heading frame must isolate golden geometry from host font metrics"
    );
}

#[test]
fn hand_built_scene_geometry_matches_reviewed_golden() {
    let mut actual = String::new();
    ratio_split(&mut actual);
    grow_row(&mut actual);
    overlapping_stack(&mut actual);
    labeled_form(&mut actual);

    assert_text_golden(
        &actual,
        include_str!("goldens/synthetic-scenes.txt"),
        GOLDEN_PATH,
    );
}
