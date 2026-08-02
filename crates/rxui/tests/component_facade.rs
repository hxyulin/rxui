//! Public facade coverage for the component-native API.

use rxui::{
    Component, ComponentContext, Theme, View, button, column, geometry::LogicalSize, label,
};
use rxui_test_support::Harness;

#[derive(Clone)]
enum Action {
    Increment,
}

struct Counter(usize);

impl Component for Counter {
    type Action = Action;
    type Effect = ();

    fn update(&mut self, action: Self::Action, _context: &mut ComponentContext<'_, ()>) {
        match action {
            Action::Increment => self.0 += 1,
        }
    }

    fn view(&self, _theme: &Theme) -> View<Self::Action> {
        column((
            label(format!("Count: {}", self.0)),
            button("Increment", Action::Increment),
        ))
    }
}

#[test]
fn component_api_is_the_rxui_root_and_prelude() {
    fn prelude_view<Action: Clone + 'static>(action: Action) -> rxui::prelude::View<Action> {
        rxui::prelude::button("Prelude", action)
    }

    let mut harness = Harness::new(Counter(0), LogicalSize::new(320.0, 200.0)).unwrap();
    harness.dispatch(Action::Increment);
    assert_eq!(harness.component().0, 1);
    let _ = prelude_view(Action::Increment);
}
