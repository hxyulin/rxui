//! Vector icon validation and retained semantics.

use astrelis_core::geometry::LogicalSize;
use astrelis_paint::Path;
use rxui_next::{Component, ComponentContext, ComponentHost, Icon, IconSpec, Theme, icon, icons};

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

    fn view(&self, _theme: &Theme) -> rxui_next::View<()> {
        icon(IconSpec::new(icons::search()).size(20.0).label("Search"))
    }
}

#[test]
fn labeled_icon_publishes_image_semantics_at_requested_size() {
    let host =
        ComponentHost::new(SearchIcon, LogicalSize::new(100.0, 100.0), Theme::dark()).unwrap();
    let icon = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Search")
        .unwrap();
    assert_eq!(icon.bounds.size, LogicalSize::new(20.0, 20.0));
}
