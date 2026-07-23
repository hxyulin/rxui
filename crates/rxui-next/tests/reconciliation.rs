//! Component and keyed reconciliation tests.

use astrelis_core::geometry::LogicalSize;
use astrelis_platform::{
    DeviceId, ElementState, Key, KeyLocation, KeyboardInput, Modifiers, PhysicalKey,
};
use rxui_next::{
    Component, ComponentContext, ComponentHost, PropertyField, Theme, column, label, property_grid,
    text_field,
};

#[derive(Clone)]
enum Action {
    SetValue(String),
    Save,
    Reverse,
}

#[derive(Debug, PartialEq, Eq)]
enum Effect {
    Saved,
}

struct Properties {
    fields: Vec<PropertyField>,
}

impl Component for Properties {
    type Action = Action;
    type Effect = Effect;

    fn update(&mut self, action: Action, context: &mut ComponentContext<'_, Effect>) {
        match action {
            Action::SetValue(value) => self.fields[1].value = value,
            Action::Save => context.emit(Effect::Saved),
            Action::Reverse => {
                self.fields.reverse();
            }
        }
    }

    fn view(&self, _theme: &Theme) -> rxui_next::View<Action> {
        column(vec![
            label("Inspector").keyed("title"),
            property_grid(&self.fields).keyed("properties"),
        ])
    }
}

#[test]
fn one_property_change_mutates_only_the_field_path() {
    let component = Properties {
        fields: (0..100)
            .map(|id| PropertyField {
                id,
                label: format!("Property {id}"),
                value: id.to_string(),
            })
            .collect(),
    };
    let mut host =
        ComponentHost::new(component, LogicalSize::new(800.0, 700.0), Theme::dark()).unwrap();
    let stats = host
        .dispatch(Action::SetValue("changed".into()))
        .unwrap()
        .stats;
    assert!(stats.rebuilt_fragments <= 4, "{stats:?}");
}

#[test]
fn typed_effects_leave_the_local_action_channel() {
    let component = Properties { fields: Vec::new() };
    let mut host =
        ComponentHost::new(component, LogicalSize::new(800.0, 700.0), Theme::dark()).unwrap();
    host.dispatch(Action::Save).unwrap();
    assert_eq!(
        host.drain_effects().collect::<Vec<_>>(),
        vec![Effect::Saved]
    );
}

#[test]
fn keyed_moves_preserve_retained_field_identity() {
    let component = Properties {
        fields: (0..4)
            .map(|id| PropertyField {
                id,
                label: format!("Property {id}"),
                value: id.to_string(),
            })
            .collect(),
    };
    let mut host =
        ComponentHost::new(component, LogicalSize::new(800.0, 700.0), Theme::dark()).unwrap();
    let before = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .filter(|node| node.data.label.starts_with("Property "))
        .map(|node| (node.data.label, node.id))
        .collect::<std::collections::HashMap<_, _>>();
    host.dispatch(Action::Reverse).unwrap();
    let after = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .filter(|node| node.data.label.starts_with("Property "))
        .map(|node| (node.data.label, node.id))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(before, after);
}

#[test]
fn duplicate_keys_fail_deterministically() {
    struct Duplicate;
    impl Component for Duplicate {
        type Action = ();
        type Effect = ();

        fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

        fn view(&self, _theme: &Theme) -> rxui_next::View<()> {
            column(vec![label("a").keyed("same"), label("b").keyed("same")])
        }
    }
    assert!(ComponentHost::new(Duplicate, LogicalSize::new(100.0, 100.0), Theme::dark()).is_err());
}

#[derive(Clone)]
enum FieldAction {
    Changed(String),
}

#[derive(Clone)]
enum FormAction {
    Field(FieldAction),
}

struct Form {
    name: String,
}

impl Component for Form {
    type Action = FormAction;
    type Effect = ();

    fn update(&mut self, action: FormAction, _context: &mut ComponentContext<'_, ()>) {
        match action {
            FormAction::Field(FieldAction::Changed(value)) => self.name = value,
        }
    }

    fn view(&self, _theme: &Theme) -> rxui_next::View<FormAction> {
        text_field("Name", self.name.clone(), FieldAction::Changed).map_action(FormAction::Field)
    }
}

#[test]
fn mapped_local_field_action_edits_and_reconciles_without_recreation() {
    let mut host = ComponentHost::new(
        Form {
            name: "Astrelis".into(),
        },
        LogicalSize::new(400.0, 100.0),
        Theme::dark(),
    )
    .unwrap();
    let semantic = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Name")
        .unwrap();
    let id = semantic.id;
    let point = astrelis_core::geometry::LogicalPoint::new(
        semantic.bounds.origin.x + semantic.bounds.size.width - 10.0,
        semantic.bounds.origin.y + 10.0,
    );
    host.input(rxui_next::core::UiInput::PointerPressed(point))
        .unwrap();
    host.input(rxui_next::core::UiInput::Keyboard {
        input: KeyboardInput {
            device_id: DeviceId(1),
            physical_key: PhysicalKey::Unidentified,
            logical_key: Key::Character("!".into()),
            text: Some("!".into()),
            location: KeyLocation::Standard,
            state: ElementState::Pressed,
            repeat: false,
            synthetic: false,
        },
        modifiers: Modifiers::default(),
    })
    .unwrap();
    assert_eq!(host.component().name, "Astrelis!");
    let after = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Name")
        .unwrap();
    assert_eq!(after.id, id);
    assert_eq!(after.data.value.as_deref(), Some("Astrelis!"));
}
