//! Reviewed old/component synthetic layout goldens.

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_core::{
    Alignment as LegacyAlignment, Insets, LayoutStyle, Length, Theme as LegacyTheme, Ui as LegacyUi,
};
use astrelis_ui_testing::{UiHarness, deterministic_font_database};
use rxui::{
    ButtonStyle, Component, ComponentContext, ComponentHost, ContainerStyle, Space, Theme, View,
    button, button_with, checkbox, column_with, label, row_with, slider, text_field,
};
use rxui_testing::differential::{SemanticScene, compare_geometry, differential_snapshot};
use rxui_testing::golden::assert_text_golden;

const FIXED_VIEWPORT: LogicalSize = LogicalSize::new(420.0, 96.0);
const FORM_VIEWPORT: LogicalSize = LogicalSize::new(420.0, 360.0);

fn legacy_fixed_row() -> UiHarness<()> {
    let mut ui = LegacyUi::new(deterministic_font_database(), LegacyTheme::dark());
    let padding = ui.add_padding(ui.root(), Insets::all(16.0)).unwrap();
    let row = ui.add_row(padding).unwrap();
    ui.set_flex(row, 12.0, LegacyAlignment::Center).unwrap();
    for text in ["One", "Two", "Three"] {
        let button = ui.add_button(row, text).unwrap();
        ui.set_layout(
            button,
            LayoutStyle {
                width: Length::Px(100.0),
                height: Length::Px(32.0),
                ..LayoutStyle::default()
            },
        )
        .unwrap();
    }
    UiHarness::with_viewport(ui, FIXED_VIEWPORT, 1.0)
}

struct FixedRow;

impl Component for FixedRow {
    type Action = ();
    type Effect = ();

    fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> View<()> {
        row_with(
            ContainerStyle::new().gap(Space::Md).padding(Space::Lg),
            ["One", "Two", "Three"].map(|text| {
                button_with(
                    text,
                    (),
                    ButtonStyle::standard().size(LogicalSize::new(100.0, 32.0)),
                )
            }),
        )
    }
}

fn legacy_form() -> UiHarness<()> {
    let mut ui = LegacyUi::new(deterministic_font_database(), LegacyTheme::dark());
    let padding = ui.add_padding(ui.root(), Insets::all(16.0)).unwrap();
    let column = ui.add_column(padding).unwrap();
    ui.set_flex(column, 12.0, LegacyAlignment::Stretch).unwrap();
    ui.add_label(column, "Account").unwrap();
    let field = ui.add_text_field(column, "Astrelis").unwrap();
    ui.set_placeholder(field, "Profile").unwrap();
    let checkbox = ui.add_checkbox(column, true).unwrap();
    ui.set_semantic_label(checkbox, "Notifications").unwrap();
    let slider = ui.add_slider(column, 0.0, 10.0, 1.0, 6.0).unwrap();
    ui.set_semantic_label(slider, "Scale").unwrap();
    ui.add_button(column, "Apply").unwrap();
    UiHarness::with_viewport(ui, FORM_VIEWPORT, 1.0)
}

struct Form;

impl Component for Form {
    type Action = ();
    type Effect = ();

    fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> View<()> {
        column_with(
            ContainerStyle::new().gap(Space::Md).padding(Space::Lg),
            (
                label("Account"),
                text_field("Profile", "Astrelis", |_| ()),
                checkbox("Notifications", true, |_| ()),
                slider("Scale", 6.0, 0.0..=10.0, |_| ()),
                button("Apply", ()),
            ),
        )
    }
}

fn legacy_scene(mut harness: UiHarness<()>) -> SemanticScene {
    SemanticScene::from_legacy(&harness.semantics().unwrap())
}

fn component_scene<C: Component>(component: C, viewport: LogicalSize) -> SemanticScene {
    let host = ComponentHost::new(component, viewport, Theme::dark()).unwrap();
    SemanticScene::from_component(&host.ui().semantic_snapshot())
}

#[test]
fn fixed_row_matches_legacy_geometry_and_reviewed_golden() {
    let legacy = legacy_scene(legacy_fixed_row());
    let next = component_scene(FixedRow, FIXED_VIEWPORT);
    compare_geometry(&legacy, &next, 0.75).unwrap_or_else(|differences| {
        panic!("{differences}\n\n{}", differential_snapshot(&legacy, &next))
    });
    assert_text_golden(
        &differential_snapshot(&legacy, &next),
        include_str!("goldens/fixed-row.txt"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/goldens/fixed-row.txt"),
    );
}

#[test]
fn intrinsic_form_has_a_reviewed_cross_implementation_golden() {
    let legacy = legacy_scene(legacy_form());
    let next = component_scene(Form, FORM_VIEWPORT);
    assert_text_golden(
        &differential_snapshot(&legacy, &next),
        include_str!("goldens/intrinsic-form.txt"),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/goldens/intrinsic-form.txt"
        ),
    );
}

#[test]
fn component_form_keeps_controls_usable_non_overlapping_and_on_screen() {
    let next = component_scene(Form, FORM_VIEWPORT);
    for landmark in &next.landmarks {
        assert!(landmark.bounds.size.width > 0.0, "{landmark:?}");
        assert!(landmark.bounds.size.height >= 14.0, "{landmark:?}");
        assert!(landmark.bounds.origin.x >= 0.0, "{landmark:?}");
        assert!(landmark.bounds.origin.y >= 0.0, "{landmark:?}");
        assert!(
            landmark.bounds.origin.x + landmark.bounds.size.width <= FORM_VIEWPORT.width,
            "{landmark:?}"
        );
        assert!(
            landmark.bounds.origin.y + landmark.bounds.size.height <= FORM_VIEWPORT.height,
            "{landmark:?}"
        );
        if matches!(
            landmark.role.as_str(),
            "Button" | "Checkbox" | "Slider" | "TextField"
        ) {
            assert!(landmark.bounds.size.height >= 24.0, "{landmark:?}");
        }
    }
    for pair in next.landmarks.windows(2) {
        let previous_bottom = pair[0].bounds.origin.y + pair[0].bounds.size.height;
        assert!(
            previous_bottom <= pair[1].bounds.origin.y,
            "overlapping landmarks: {:?} and {:?}",
            pair[0],
            pair[1]
        );
    }
}
