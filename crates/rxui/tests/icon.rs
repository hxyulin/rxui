//! Vector icon validation and retained semantics.

use astrelis_core::geometry::LogicalSize;
use astrelis_paint::Path;
use rxui::{Component, ComponentContext, Icon, IconSpec, Theme, icon, icon_button, icons};
use rxui_test_support::Harness;

#[test]
fn icon_rejects_empty_or_invalid_geometry() {
    assert!(Icon::new(LogicalSize::ZERO, Path::builder().finish()).is_err());
    assert!(Icon::new(LogicalSize::new(24.0, 24.0), Path::builder().finish()).is_err());
}

struct SearchIcon;

impl Component for SearchIcon {
    type Action = ();
    type Effect = ();

    fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> rxui::View<()> {
        icon(IconSpec::new(icons::search()).size(20.0).label("Search"))
    }
}

#[test]
fn labeled_icon_publishes_image_semantics_at_requested_size() {
    let harness = Harness::new(SearchIcon, LogicalSize::new(100.0, 100.0)).unwrap();
    assert_eq!(harness.bounds("Search").size, LogicalSize::new(20.0, 20.0));
}

struct SearchButton;

impl Component for SearchButton {
    type Action = ();
    type Effect = ();

    fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> rxui::View<()> {
        icon_button(icons::search(), "Search", ())
    }
}

#[test]
fn compact_icon_button_keeps_button_semantics_without_painting_its_label() {
    let harness = Harness::new(SearchButton, LogicalSize::new(100.0, 100.0)).unwrap();
    let button = harness.find("Search");
    assert_eq!(button.data.role, astrelis_ui_next::SemanticRole::Button);
    assert_eq!(button.bounds.size, LogicalSize::new(30.0, 30.0));
}
