//! RXUI presets and compatibility exports for deterministic Astrelis UI testing.

#![warn(missing_docs)]

pub use astrelis_ui_testing::{SnapshotBundle, UiHarness, deterministic_font_database};

/// Returns the dark RXUI theme pinned to the bundled deterministic font.
pub fn deterministic_theme() -> astrelis_ui_core::Theme {
    astrelis_ui_core::Theme {
        font_families: vec![astrelis_text::FontFamily::Named("Noto Sans".into())],
        ..astrelis_ui_core::Theme::dark()
    }
}

#[cfg(test)]
mod tests {
    use astrelis_ui_core::{EventFilter, SemanticRole, Ui};

    use super::*;

    fn sample() -> Ui<i32> {
        let mut ui = Ui::new(deterministic_font_database(), deterministic_theme());
        let button = ui.add_button(ui.root(), "Save").unwrap();
        ui.listen(button, None, EventFilter::Activate, |context, _| {
            context.emit(7)
        })
        .unwrap();
        ui
    }

    #[test]
    fn compatibility_facade_preserves_actions_and_snapshots() {
        let mut harness = UiHarness::new(sample());
        harness.activate(SemanticRole::Button, "Save").unwrap();
        assert_eq!(harness.drain_messages().collect::<Vec<_>>(), vec![7]);

        let bundle = UiHarness::new(sample()).snapshot_bundle().unwrap();
        assert_eq!(
            bundle.semantics,
            include_str!("snapshots/basic.semantics.txt")
        );
        assert_eq!(
            bundle.inspection,
            include_str!("snapshots/basic.inspection.txt")
        );
        assert_eq!(
            bundle.display_list,
            include_str!("snapshots/basic.display-list.txt")
        );
    }
}
