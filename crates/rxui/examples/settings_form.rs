//! Controlled settings form with a reusable local action scope.

use astrelis_core::geometry::LogicalSize;
use rxui::{
    Component, ComponentContext, ComponentHost, ComponentWithProps, PropertyField, Theme, View,
    button, column, component, editable_property_grid, label, row,
};

#[derive(Clone)]
enum FieldAction {
    Changed(u64, String),
}

enum FieldEffect {
    Changed(u64, String),
}

#[derive(Clone, PartialEq)]
struct GraphicsProps {
    fields: Vec<PropertyField>,
}

struct GraphicsFields {
    fields: Vec<PropertyField>,
}

impl Component for GraphicsFields {
    type Action = FieldAction;
    type Effect = FieldEffect;

    fn update(&mut self, action: FieldAction, context: &mut ComponentContext<'_, FieldEffect>) {
        match action {
            FieldAction::Changed(id, value) => context.emit(FieldEffect::Changed(id, value)),
        }
    }

    fn view(&self, _theme: &Theme) -> View<FieldAction> {
        editable_property_grid(&self.fields, FieldAction::Changed)
    }
}

impl ComponentWithProps for GraphicsFields {
    type Props = GraphicsProps;

    fn create(props: &GraphicsProps) -> Self {
        Self {
            fields: props.fields.clone(),
        }
    }

    fn changed(&mut self, props: &GraphicsProps) {
        self.fields.clone_from(&props.fields);
    }
}

#[derive(Clone)]
enum Action {
    FieldChanged(u64, String),
    Apply,
    Reset,
}

#[derive(Debug, PartialEq, Eq)]
enum Effect {
    Apply {
        renderer: String,
        resolution: String,
    },
}

struct Settings {
    fields: Vec<PropertyField>,
}

impl Settings {
    fn defaults() -> Vec<PropertyField> {
        vec![
            PropertyField {
                id: 1,
                label: "Renderer".into(),
                value: "WebGPU".into(),
            },
            PropertyField {
                id: 2,
                label: "Resolution".into(),
                value: "2560×1440".into(),
            },
        ]
    }
}

impl Component for Settings {
    type Action = Action;
    type Effect = Effect;

    fn update(&mut self, action: Action, context: &mut ComponentContext<'_, Effect>) {
        match action {
            Action::FieldChanged(id, value) => {
                if let Some(field) = self.fields.iter_mut().find(|field| field.id == id) {
                    field.value = value;
                }
            }
            Action::Apply => context.emit(Effect::Apply {
                renderer: self.fields[0].value.clone(),
                resolution: self.fields[1].value.clone(),
            }),
            Action::Reset => self.fields = Self::defaults(),
        }
    }

    fn view(&self, _theme: &Theme) -> View<Action> {
        column((
            label("Graphics").key("title"),
            component::<GraphicsFields, Action>(
                GraphicsProps {
                    fields: self.fields.clone(),
                },
                |effect| match effect {
                    FieldEffect::Changed(id, value) => Action::FieldChanged(id, value),
                },
            )
            .key("fields"),
            row((
                button("Reset", Action::Reset).key("reset"),
                button("Apply", Action::Apply).key("apply"),
            ))
            .key("actions"),
        ))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut host = ComponentHost::new(
        Settings {
            fields: Settings::defaults(),
        },
        LogicalSize::new(640.0, 320.0),
        Theme::dark(),
    )?;

    host.dispatch(Action::FieldChanged(2, "1920×1080".into()))?;
    host.dispatch(Action::Apply)?;

    assert_eq!(
        host.drain_effects().collect::<Vec<_>>(),
        vec![Effect::Apply {
            renderer: "WebGPU".into(),
            resolution: "1920×1080".into(),
        }]
    );
    Ok(())
}
