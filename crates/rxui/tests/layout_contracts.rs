//! Absolute layout contracts for padding, gaps, and intrinsic control sizing.
//!
//! These scenes used to be proved by diffing the component engine against the
//! old retained engine within a 0.75 logical-pixel tolerance. The oracle is
//! gone: a compose-equivalence property test in the engine covers what it was
//! really protecting, and a tolerance band is a weaker statement than the exact
//! offsets a fixed-size scene is entitled to. The fixed row therefore asserts
//! its geometry outright, and the intrinsic form asserts the usability policy
//! that had no cross-implementation answer in the first place.

use astrelis_core::geometry::LogicalSize;
use rxui::{
    ButtonStyle, Component, ComponentContext, ContainerStyle, Space, Theme, View, button,
    button_with, checkbox, column_with, label, row_with, slider, text_field,
};
use rxui_test_support::{Harness, assert_text_golden};

const FIXED_VIEWPORT: LogicalSize = LogicalSize::new(420.0, 96.0);
const FORM_VIEWPORT: LogicalSize = LogicalSize::new(420.0, 360.0);

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

#[test]
fn fixed_row_places_explicitly_sized_buttons_at_exact_offsets() {
    let harness = Harness::new(FixedRow, FIXED_VIEWPORT).unwrap();

    // `Space::Lg` padding puts the content box at 16, and `Space::Md` gaps add
    // 12 between the 100-wide buttons: 16, 16+112, 16+224.
    for (label, x) in [("One", 16.0), ("Two", 128.0), ("Three", 240.0)] {
        let bounds = harness.bounds(label);
        assert_eq!(bounds.origin.x, x, "{label} x");
        assert_eq!(bounds.origin.y, 16.0, "{label} y");
        assert_eq!(bounds.size, LogicalSize::new(100.0, 32.0), "{label} size");
    }

    let labels = harness
        .scene()
        .landmarks
        .iter()
        .map(|landmark| landmark.label.clone())
        .collect::<Vec<_>>();
    assert_eq!(labels, ["One", "Two", "Three"]);
}

/// Pins the resolved geometry of intrinsically sized controls.
///
/// Nothing else in the suite fixes a text field's, checkbox's, slider's, or
/// text label's default size, so a silent change to any of them would otherwise
/// only be caught by the far looser policy assertions below.
#[test]
fn intrinsic_form_sizing_matches_reviewed_golden() {
    let harness = Harness::new(Form, FORM_VIEWPORT).unwrap();
    assert_text_golden(
        &harness.snapshot(),
        include_str!("goldens/intrinsic-form.txt"),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/goldens/intrinsic-form.txt"
        ),
    );
}

#[test]
fn intrinsic_form_keeps_controls_usable_non_overlapping_and_on_screen() {
    let harness = Harness::new(Form, FORM_VIEWPORT).unwrap();
    let landmarks = harness.scene().landmarks;

    for landmark in &landmarks {
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

    for pair in landmarks.windows(2) {
        let previous_bottom = pair[0].bounds.origin.y + pair[0].bounds.size.height;
        assert!(
            previous_bottom <= pair[1].bounds.origin.y,
            "overlapping landmarks: {:?} and {:?}",
            pair[0],
            pair[1]
        );
    }
}
