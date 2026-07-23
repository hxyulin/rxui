//! Controlled settings form with a reusable local action scope.

use astrelis_core::geometry::LogicalSize;
use rxui_next::{
    Component, ComponentContext, ComponentHost, PropertyField, Theme, View, button, column,
    editable_property_grid, label, row,
};

#[derive(Clone)]
enum FieldAction {
    Changed(u64, String),
}

fn graphics_fields(fields: &[PropertyField]) -> View<FieldAction> {
    editable_property_grid(fields, FieldAction::Changed)
}

#[derive(Clone)]
enum Action {
    Field(FieldAction),
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
            Action::Field(FieldAction::Changed(id, value)) => {
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
            graphics_fields(&self.fields)
                .map_action(Action::Field)
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

    host.dispatch(Action::Field(FieldAction::Changed(2, "1920×1080".into())))?;
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
