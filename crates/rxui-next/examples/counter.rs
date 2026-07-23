//! Minimal typed component using tuple children and application effects.

use astrelis_core::geometry::LogicalSize;
use rxui_next::{
    Component, ComponentContext, ComponentHost, Theme, View, button, column, label, row,
};

#[derive(Clone)]
enum Action {
    Decrement,
    Increment,
    Save,
}

#[derive(Debug, PartialEq, Eq)]
enum Effect {
    SaveValue(i32),
}

struct Counter {
    value: i32,
}

impl Component for Counter {
    type Action = Action;
    type Effect = Effect;

    fn update(&mut self, action: Action, context: &mut ComponentContext<'_, Effect>) {
        match action {
            Action::Decrement => self.value -= 1,
            Action::Increment => self.value += 1,
            Action::Save => context.emit(Effect::SaveValue(self.value)),
        }
    }

    fn view(&self, _theme: &Theme) -> View<Action> {
        column((
            label(format!("Value: {}", self.value)).key("value"),
            row((
                button("−", Action::Decrement).key("decrement"),
                button("+", Action::Increment).key("increment"),
                button("Save", Action::Save).key("save"),
            ))
            .key("actions"),
        ))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut host = ComponentHost::new(
        Counter { value: 0 },
        LogicalSize::new(360.0, 160.0),
        Theme::dark(),
    )?;

    host.dispatch(Action::Increment)?;
    host.dispatch(Action::Increment)?;
    host.dispatch(Action::Save)?;

    assert_eq!(
        host.drain_effects().collect::<Vec<_>>(),
        vec![Effect::SaveValue(2)]
    );
    println!("counter value: {}", host.component().value);
    Ok(())
}
