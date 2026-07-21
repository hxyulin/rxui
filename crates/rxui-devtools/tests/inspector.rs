//! End-to-end inspector flows exercised through the public API.
//!
//! A host application tree is driven exactly as a consumer would drive it: the
//! semantic testing harness activates real inspector controls, the emitted
//! [`InspectorAction`] messages are routed back through [`UiInspector::apply`],
//! and the observable host/inspector state is asserted afterwards.

use astrelis_ui_core::{
    Button, ElementHandle, ElementId, Insets, Label, Padding, SemanticAction, SemanticNode,
    SemanticRole, Ui, Visibility,
};
use rxui_devtools::{InspectorAction, InspectorOptions, UiInspector};
use rxui_testing::{UiHarness, deterministic_font_database, deterministic_theme};

#[derive(Clone)]
enum Message {
    Inspector(InspectorAction),
}

/// A small host application with an inspector mounted over it.
struct Fixture {
    harness: UiHarness<Message>,
    inspector: UiInspector<Message>,
    button: ElementHandle<Button>,
    label: ElementHandle<Label>,
    padding: ElementHandle<Padding>,
}

fn fixture(options: InspectorOptions) -> Fixture {
    let mut ui: Ui<Message> = Ui::new(deterministic_font_database(), deterministic_theme());
    let column = ui.add_column(ui.root()).unwrap();
    let label = ui.add_label(column, "Hello").unwrap();
    let button = ui.add_button(column, "Submit").unwrap();
    let padding = ui.add_padding(column, Insets::all(7.0)).unwrap();
    ui.add_label(padding, "Padded").unwrap();
    let mut harness = UiHarness::new(ui);
    let inspector = UiInspector::new(harness.ui_mut(), options, Message::Inspector).unwrap();
    Fixture {
        harness,
        inspector,
        button,
        label,
        padding,
    }
}

fn open_editing_options() -> InspectorOptions {
    InspectorOptions {
        initially_open: true,
        allow_editing: true,
        ..InspectorOptions::default()
    }
}

/// Routes every queued application message back through the inspector, the
/// way a host application's update loop would.
fn pump(fixture: &mut Fixture) {
    loop {
        let actions: Vec<InspectorAction> = fixture
            .harness
            .drain_messages()
            .map(|Message::Inspector(action)| action)
            .collect();
        if actions.is_empty() {
            return;
        }
        for action in actions {
            fixture
                .inspector
                .apply(fixture.harness.ui_mut(), action)
                .unwrap();
        }
    }
}

fn find_node(
    node: &SemanticNode,
    predicate: &impl Fn(&SemanticNode) -> bool,
) -> Option<SemanticNode> {
    if predicate(node) {
        return Some(node.clone());
    }
    node.children
        .iter()
        .find_map(|child| find_node(child, predicate))
}

fn find_semantic(
    fixture: &mut Fixture,
    predicate: impl Fn(&SemanticNode) -> bool,
) -> Option<SemanticNode> {
    let root = fixture.harness.semantics().unwrap();
    find_node(&root, &predicate)
}

fn activate_by_id(fixture: &mut Fixture, id: ElementId) {
    fixture
        .harness
        .ui_mut()
        .perform_semantic_action(id, SemanticAction::Activate)
        .unwrap();
}

fn all_labels(fixture: &mut Fixture) -> Vec<String> {
    fn visit(node: &SemanticNode, output: &mut Vec<String>) {
        output.push(node.label.clone());
        for child in &node.children {
            visit(child, output);
        }
    }
    let root = fixture.harness.semantics().unwrap();
    let mut output = Vec::new();
    visit(&root, &mut output);
    output
}

#[test]
fn launcher_and_close_button_toggle_the_panel() {
    let mut fixture = fixture(InspectorOptions::default());
    assert!(!fixture.inspector.is_open());
    assert!(
        find_semantic(&mut fixture, |node| node.role == SemanticRole::Dialog).is_none(),
        "the closed panel must not appear in application semantics"
    );

    fixture
        .harness
        .activate(SemanticRole::Button, "Inspect")
        .unwrap();
    pump(&mut fixture);
    assert!(fixture.inspector.is_open());
    let dialog = find_semantic(&mut fixture, |node| node.role == SemanticRole::Dialog)
        .expect("the open panel must expose its dialog role");
    assert_eq!(dialog.description.as_deref(), Some("Retained UI inspector"));
    assert!(
        fixture
            .harness
            .find(SemanticRole::Button, "Inspect")
            .unwrap()
            .is_none(),
        "the launcher must hide while the panel is open"
    );

    fixture
        .harness
        .activate(SemanticRole::Button, "Close inspector")
        .unwrap();
    pump(&mut fixture);
    assert!(!fixture.inspector.is_open());
    assert!(
        fixture
            .harness
            .find(SemanticRole::Button, "Inspect")
            .unwrap()
            .is_some(),
        "the launcher must return once the panel closes"
    );
}

#[test]
fn tree_rows_reflect_host_kinds_roles_and_labels() {
    let mut fixture = fixture(InspectorOptions {
        initially_open: true,
        ..InspectorOptions::default()
    });
    pump(&mut fixture);
    let labels = all_labels(&mut fixture);
    // Tree rows render the element kind plus the quoted semantic label.
    assert!(labels.iter().any(|label| label == "\"Submit\""));
    assert!(labels.iter().any(|label| label == "\"Hello\""));
    assert!(labels.iter().any(|label| label == "Padding"));
    assert!(labels.iter().any(|label| label == "Column"));
}

#[test]
fn search_field_input_filters_tree_rows() {
    let mut fixture = fixture(InspectorOptions {
        initially_open: true,
        ..InspectorOptions::default()
    });
    let labels = all_labels(&mut fixture);
    assert!(labels.iter().any(|label| label == "\"Hello\""));

    fixture
        .harness
        .perform(
            SemanticRole::TextField,
            "Filter by kind, role, or label",
            SemanticAction::SetText("submit".into()),
        )
        .unwrap();
    pump(&mut fixture);

    let labels = all_labels(&mut fixture);
    assert!(
        labels.iter().any(|label| label == "\"Submit\""),
        "the filter must keep the matching row"
    );
    assert!(
        !labels.iter().any(|label| label == "\"Hello\""),
        "the filter must drop rows that match neither kind, role, nor label"
    );
}

#[test]
fn selecting_an_element_exposes_its_properties() {
    let mut fixture = fixture(InspectorOptions {
        initially_open: true,
        ..InspectorOptions::default()
    });
    let button = fixture.button.id();
    fixture
        .inspector
        .apply(fixture.harness.ui_mut(), InspectorAction::Select(button))
        .unwrap();
    assert_eq!(fixture.inspector.selected(), Some(button));

    let labels = all_labels(&mut fixture);
    for section in ["BOX MODEL", "LAYOUT", "COMPUTED", "PAINT", "SEMANTICS"] {
        assert!(
            labels.iter().any(|label| label == section),
            "details must render the {section} section"
        );
    }
    assert!(
        labels.iter().any(|label| label == "role"),
        "details must render the semantic role row"
    );
    assert!(
        labels.iter().any(|label| label == "›"),
        "breadcrumbs must render the ancestor chain"
    );
}

#[test]
fn enabled_checkbox_edit_flows_back_into_the_host() {
    let mut fixture = fixture(open_editing_options());
    let button = fixture.button.id();
    fixture
        .inspector
        .apply(fixture.harness.ui_mut(), InspectorAction::Select(button))
        .unwrap();

    let checkbox = find_semantic(&mut fixture, |node| {
        node.role == SemanticRole::Checkbox && node.value.as_deref() == Some("true")
    })
    .expect("editing details must offer the enabled checkbox");
    activate_by_id(&mut fixture, checkbox.id);
    pump(&mut fixture);

    let inspected = fixture
        .harness
        .ui_mut()
        .inspect_element(fixture.button)
        .unwrap();
    assert!(
        !inspected.enabled,
        "the checkbox edit must disable the host button"
    );
    assert!(
        find_semantic(&mut fixture, |node| {
            node.role == SemanticRole::Checkbox && node.value.as_deref() == Some("false")
        })
        .is_some(),
        "the rebuilt details must reseed the checkbox from the edited state"
    );
}

#[test]
fn visibility_dropdown_edit_hides_the_host_element() {
    let mut fixture = fixture(open_editing_options());
    let label = fixture.label.id();
    fixture
        .inspector
        .apply(fixture.harness.ui_mut(), InspectorAction::Select(label))
        .unwrap();

    // Open the visibility combo box, then choose "Hidden" from its popup.
    fixture
        .harness
        .activate(SemanticRole::Button, "Visible")
        .unwrap();
    fixture
        .harness
        .activate(SemanticRole::MenuItem, "Hidden")
        .unwrap();
    pump(&mut fixture);

    let inspected = fixture
        .harness
        .ui_mut()
        .inspect_element(fixture.label)
        .unwrap();
    assert_eq!(inspected.visibility, Visibility::Hidden);
    assert!(
        fixture
            .harness
            .find(SemanticRole::Button, "Hidden")
            .unwrap()
            .is_some(),
        "the rebuilt combo box must show the edited visibility"
    );
}

#[test]
fn dock_menu_activation_moves_the_panel() {
    let mut fixture = fixture(InspectorOptions {
        initially_open: true,
        ..InspectorOptions::default()
    });
    assert_eq!(
        fixture.harness.ui_mut().content_inset(),
        Insets {
            right: 340.0,
            ..Insets::default()
        }
    );

    let anchor = find_semantic(&mut fixture, |node| {
        node.description.as_deref() == Some("Dock side")
    })
    .expect("the panel header must expose the dock menu anchor");
    activate_by_id(&mut fixture, anchor.id);
    let choice = fixture
        .harness
        .find(SemanticRole::MenuItem, "Dock bottom")
        .unwrap()
        .expect("the dock menu must list its sides once opened");
    activate_by_id(&mut fixture, choice.id);
    pump(&mut fixture);

    assert_eq!(
        fixture.harness.ui_mut().content_inset(),
        Insets {
            bottom: 320.0,
            ..Insets::default()
        },
        "docking to the bottom must swap the reserved content inset"
    );
}

#[test]
fn typed_text_without_submitting_does_not_commit() {
    let mut fixture = fixture(open_editing_options());
    let padding = fixture.padding.id();
    fixture
        .inspector
        .apply(fixture.harness.ui_mut(), InspectorAction::Select(padding))
        .unwrap();

    let field = find_semantic(&mut fixture, |node| {
        node.role == SemanticRole::TextField && node.value.as_deref() == Some("7 7 7 7")
    })
    .expect("editing details must seed the padding editor from the host");
    fixture
        .harness
        .ui_mut()
        .perform_semantic_action(field.id, SemanticAction::SetText("50%".into()))
        .unwrap();
    pump(&mut fixture);

    // Editors are commit-based: replacing the text without submitting must
    // leave the host untouched.
    let inspected = fixture
        .harness
        .ui_mut()
        .inspect_element(fixture.padding)
        .unwrap();
    assert_eq!(inspected.resolved_padding, Insets::all(7.0));
}
