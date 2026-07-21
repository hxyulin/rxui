//! Commit-based property editing driven through the scripted platform backend.
//!
//! The inspector's editors commit on Enter (`TextSubmitted`), which only the
//! keyboard path produces, so these tests run the deterministic scripted
//! event loop: seed an editor field, press Enter, route the emitted
//! [`InspectorAction`] messages back through [`UiInspector::apply`], and
//! assert whether the host mutated. Invalid entries (percent insets, non-hex
//! colors) must be rejected without mutating and must snap the editor back.

use std::io;

use astrelis_app::{App, AppContext, Runtime, RuntimeConfig};
use astrelis_core::{color::Color, geometry::Size};
use astrelis_platform::{
    DeviceId, ElementState, Key, KeyLocation, KeyboardInput, NamedKey, PhysicalKey, Window,
    WindowAttributes, WindowEvent, WindowId,
};
use astrelis_platform_test::{ScriptEvent, TestRunner};
use astrelis_ui_core::{
    ElementHandle, ElementId, Insets, Label, Padding, SemanticAction, SemanticNode, SemanticRole,
    Ui, WidgetStyle,
};
use rxui_devtools::{InspectorAction, InspectorOptions, UiInspector};
use rxui_testing::{deterministic_font_database, deterministic_theme};

const SEED_COLOR: u32 = 0x112233;

#[derive(Clone)]
enum Message {
    Inspector(InspectorAction),
}

/// Which host element the test selects and edits.
#[derive(Clone, Copy)]
enum Target {
    Padding,
    Label,
}

struct EditApp {
    window: Option<Window>,
    ui: Ui<Message>,
    inspector: UiInspector<Message>,
    padding: ElementHandle<Padding>,
    label: ElementHandle<Label>,
}

impl EditApp {
    /// Builds the host and inspector, selects `target`, replaces the editor
    /// field currently showing `seed` with `typed`, and focuses it so the
    /// scripted Enter key commits the entry.
    fn new(target: Target, seed: &str, typed: &str) -> Self {
        let mut ui: Ui<Message> = Ui::new(deterministic_font_database(), deterministic_theme());
        ui.set_viewport(Size::new(800.0, 600.0), 1.0);
        let padding = ui.add_padding(ui.root(), Insets::all(7.0)).unwrap();
        ui.add_label(padding, "Padded").unwrap();
        let label = ui.add_label(ui.root(), "Styled").unwrap();
        ui.set_widget_style(
            label,
            WidgetStyle {
                foreground: Some(Color::from_hex(SEED_COLOR)),
                ..WidgetStyle::default()
            },
        )
        .unwrap();
        let mut inspector = UiInspector::new(
            &mut ui,
            InspectorOptions {
                initially_open: true,
                allow_editing: true,
                ..InspectorOptions::default()
            },
            Message::Inspector,
        )
        .unwrap();
        let selected = match target {
            Target::Padding => padding.id(),
            Target::Label => label.id(),
        };
        inspector
            .apply(&mut ui, InspectorAction::Select(selected))
            .unwrap();
        let field = editor_field(&mut ui, seed).expect("details must seed an editor field");
        ui.perform_semantic_action(field, SemanticAction::SetText(typed.into()))
            .unwrap();
        ui.perform_semantic_action(field, SemanticAction::Focus)
            .unwrap();
        Self {
            window: None,
            ui,
            inspector,
            padding,
            label,
        }
    }

    fn field_showing(&mut self, value: &str) -> Option<ElementId> {
        editor_field(&mut self.ui, value)
    }
}

impl App for EditApp {
    type Error = io::Error;

    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        self.window = Some(
            context
                .create_window(WindowAttributes::default())
                .map_err(io::Error::other)?,
        );
        Ok(())
    }

    fn window_event(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        _id: WindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        let window = self.window.as_ref().expect("window");
        self.ui
            .handle_window_event(window, &context.clipboard(), &event)
            .map_err(io::Error::other)?;
        loop {
            let actions: Vec<InspectorAction> = self
                .ui
                .drain_messages()
                .map(|Message::Inspector(action)| action)
                .collect();
            if actions.is_empty() {
                return Ok(());
            }
            for action in actions {
                self.inspector
                    .apply(&mut self.ui, action)
                    .map_err(io::Error::other)?;
            }
        }
    }
}

/// Finds the details-pane text field currently showing `value`.
fn editor_field(ui: &mut Ui<Message>, value: &str) -> Option<ElementId> {
    fn visit(node: &SemanticNode, value: &str) -> Option<ElementId> {
        if node.role == SemanticRole::TextField && node.value.as_deref() == Some(value) {
            return Some(node.id);
        }
        node.children.iter().find_map(|child| visit(child, value))
    }
    let root = ui.semantic_tree().unwrap();
    visit(&root, value)
}

fn enter_key() -> WindowEvent {
    WindowEvent::KeyboardInput(KeyboardInput {
        device_id: DeviceId(1),
        physical_key: PhysicalKey::Unidentified,
        logical_key: Key::Named(NamedKey::Enter),
        text: None,
        location: KeyLocation::Standard,
        state: ElementState::Pressed,
        repeat: false,
        synthetic: false,
    })
}

/// Runs the scripted loop: create the window, press Enter, exit.
fn press_enter(app: EditApp) -> EditApp {
    let mut runner = TestRunner::new();
    runner.push(ScriptEvent::Resumed);
    runner.push(ScriptEvent::Window(WindowId(1), enter_key()));
    runner.push(ScriptEvent::Exit);
    let (runtime, _state) = runner
        .run_return(Runtime::new(app, RuntimeConfig::default()))
        .unwrap();
    runtime.into_result().unwrap()
}

#[test]
fn submitted_pixel_padding_edit_mutates_the_host() {
    let mut app = press_enter(EditApp::new(Target::Padding, "7 7 7 7", "12"));
    let padding = app.padding;
    assert_eq!(
        app.ui.inspect_element(padding).unwrap().resolved_padding,
        Insets::all(12.0)
    );
    assert!(
        app.field_showing("12 12 12 12").is_some(),
        "the rebuilt editor must reseed from the committed insets"
    );
}

#[test]
fn submitted_percent_padding_edit_is_rejected_without_mutating() {
    let mut app = press_enter(EditApp::new(Target::Padding, "7 7 7 7", "50%"));
    let padding = app.padding;
    assert_eq!(
        app.ui.inspect_element(padding).unwrap().resolved_padding,
        Insets::all(7.0),
        "percent lengths are not valid insets and must not mutate the host"
    );
    assert!(
        app.field_showing("7 7 7 7").is_some(),
        "a rejected commit must snap the editor back to the current value"
    );
}

#[test]
fn submitted_hex_color_edit_recolors_the_host_element() {
    let mut app = press_enter(EditApp::new(Target::Label, "#112233", "#44aa66"));
    let label = app.label;
    assert_eq!(
        app.ui
            .inspect_element(label)
            .unwrap()
            .widget_style
            .foreground,
        Some(Color::from_hex(0x44aa66))
    );
}

#[test]
fn submitted_non_hex_color_edit_is_rejected_without_mutating() {
    let mut app = press_enter(EditApp::new(Target::Label, "#112233", "not-a-color"));
    let label = app.label;
    assert_eq!(
        app.ui
            .inspect_element(label)
            .unwrap()
            .widget_style
            .foreground,
        Some(Color::from_hex(SEED_COLOR)),
        "non-hex entries must not mutate the host color"
    );
    assert!(
        app.field_showing("#112233").is_some(),
        "a rejected commit must snap the editor back to the current color"
    );
}
