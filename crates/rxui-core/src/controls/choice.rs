//! Controlled single-selection groups.

use crate::{View, ViewKey, button, column, views};

/// One controlled radio-group option.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice<Value> {
    /// Stable choice identity, independent of position in the group.
    pub id: ViewKey,
    /// Domain value emitted when selected.
    pub value: Value,
    /// User-visible label.
    pub label: String,
    /// Whether the choice accepts interaction.
    pub enabled: bool,
}

impl<Value> Choice<Value> {
    /// Creates an enabled choice.
    pub fn new(id: impl Into<ViewKey>, value: Value, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            value,
            label: label.into(),
            enabled: true,
        }
    }

    /// Changes interaction enablement.
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Builds a controlled, keyed single-selection group.
pub fn radio_group<Action, Value>(
    choices: &[Choice<Value>],
    selected: Option<&Value>,
    on_selected: impl Fn(Value) -> Action + Clone + 'static,
) -> View<Action>
where
    Action: Clone + 'static,
    Value: Clone + PartialEq + 'static,
{
    column(views(choices.iter().map(|choice| {
        let marker = if selected == Some(&choice.value) {
            "●"
        } else {
            "○"
        };
        let value = choice.value.clone();
        let on_selected = on_selected.clone();
        button(format!("{marker} {}", choice.label), on_selected(value))
            .enabled(choice.enabled)
            .key(choice.id.clone())
    })))
}

#[cfg(test)]
mod tests {
    use astrelis_core::geometry::LogicalSize;

    use super::*;
    use crate::{
        Component, ComponentContext, ComponentHost, Theme,
        semantic_probe::{focused, node_labeled},
    };

    struct RadioForm {
        choices: Vec<Choice<&'static str>>,
    }

    impl Component for RadioForm {
        type Action = &'static str;
        type Effect = ();

        fn update(&mut self, _action: Self::Action, _context: &mut ComponentContext<'_, ()>) {}

        fn view(&self, _theme: &Theme) -> View<Self::Action> {
            radio_group(&self.choices, None, |value| value)
        }
    }

    #[test]
    fn radio_choices_keep_their_retained_identity_across_an_insertion() {
        let mut host = ComponentHost::new(
            RadioForm {
                choices: vec![
                    Choice::new("beta", "beta", "Beta"),
                    Choice::new("gamma", "gamma", "Gamma"),
                ],
            },
            LogicalSize::new(320.0, 240.0),
            Theme::dark(),
        )
        .unwrap();
        let beta = node_labeled(&host, "Beta");
        host.ui_mut().set_focus(Some(beta)).unwrap();
        host.refresh().unwrap();
        assert_eq!(focused(&host).id, beta);

        host.component_mut()
            .choices
            .insert(0, Choice::new("alpha", "alpha", "Alpha"));
        host.refresh().unwrap();

        // Position-keyed children would hand Beta's retained control - and its
        // focus - to the newly inserted Alpha.
        let focused = focused(&host);
        assert_eq!(focused.id, beta);
        assert!(focused.data.label.ends_with("Beta"), "{:?}", focused.data);
        assert_eq!(node_labeled(&host, "Beta"), beta);
        assert_ne!(node_labeled(&host, "Alpha"), beta);
    }
}
