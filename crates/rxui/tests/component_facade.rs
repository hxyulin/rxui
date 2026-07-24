//! Public facade coverage for the component-native API.

use astrelis_core::geometry::LogicalSize;
use rxui::{Component, ComponentContext, ComponentHost, Theme, View, button, column, label};

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

    let mut host =
        ComponentHost::new(Counter(0), LogicalSize::new(320.0, 200.0), Theme::dark()).unwrap();
    host.dispatch(Action::Increment).unwrap();
    assert_eq!(host.component().0, 1);
    let _ = prelude_view(Action::Increment);
}
