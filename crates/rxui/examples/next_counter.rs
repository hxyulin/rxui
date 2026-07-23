//! Root-facade component example used to validate the final export cutover.

use astrelis_core::geometry::LogicalSize;
use rxui::prelude::*;

#[derive(Clone)]
enum Action {
    Increment,
}

struct Counter(usize);

impl Component for Counter {
    type Action = Action;
    type Effect = ();

    fn update(&mut self, action: Action, _context: &mut ComponentContext<'_, ()>) {
        match action {
            Action::Increment => self.0 += 1,
        }
    }

    fn view(&self, _theme: &Theme) -> View<Action> {
        column((
            label(format!("Count: {}", self.0)),
            button("Increment", Action::Increment),
        ))
    }
}

fn main() {
    let mut host =
        ComponentHost::new(Counter(0), LogicalSize::new(320.0, 200.0), Theme::dark()).unwrap();
    host.dispatch(Action::Increment).unwrap();
    assert_eq!(host.component().0, 1);
}
