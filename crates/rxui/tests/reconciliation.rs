//! Component and keyed reconciliation tests.

use astrelis_core::geometry::LogicalSize;
use astrelis_platform::{
    DeviceId, ElementState, Key, KeyLocation, KeyboardInput, Modifiers, NamedKey, PhysicalKey,
};
use rxui::{
    CommandItem, CommandPaletteNavigation, Component, ComponentContext, ComponentHost,
    ComponentWithProps, PropertyField, Theme, button, checkbox, column, command_palette, component,
    label, property_grid, slider, stack, text_field,
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

    fn view(&self, _theme: &Theme) -> rxui::View<Action> {
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

        fn view(&self, _theme: &Theme) -> rxui::View<()> {
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

    fn view(&self, _theme: &Theme) -> rxui::View<FormAction> {
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
    host.input(rxui::core::UiInput::PointerPressed(point))
        .unwrap();
    host.input(rxui::core::UiInput::Keyboard {
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

#[derive(Clone, PartialEq)]
struct CounterProps {
    step: i32,
}

#[derive(Clone)]
enum CounterAction {
    Increment,
}

enum CounterEffect {
    Changed(i32),
}

struct ChildCounter {
    step: i32,
    value: i32,
}

impl Component for ChildCounter {
    type Action = CounterAction;
    type Effect = CounterEffect;

    fn update(&mut self, action: CounterAction, context: &mut ComponentContext<'_, CounterEffect>) {
        match action {
            CounterAction::Increment => {
                self.value += self.step;
                context.emit(CounterEffect::Changed(self.value));
            }
        }
    }

    fn view(&self, _theme: &Theme) -> rxui::View<CounterAction> {
        button("Increment", CounterAction::Increment)
    }
}

impl ComponentWithProps for ChildCounter {
    type Props = CounterProps;

    fn create(props: &CounterProps) -> Self {
        Self {
            step: props.step,
            value: 0,
        }
    }

    fn changed(&mut self, props: &CounterProps) {
        self.step = props.step;
    }
}

#[derive(Clone)]
enum ParentAction {
    ChildChanged(i32),
    SetStep(i32),
}

struct Parent {
    step: i32,
    child_value: i32,
}

impl Component for Parent {
    type Action = ParentAction;
    type Effect = ();

    fn update(&mut self, action: ParentAction, _context: &mut ComponentContext<'_, ()>) {
        match action {
            ParentAction::ChildChanged(value) => self.child_value = value,
            ParentAction::SetStep(step) => self.step = step,
        }
    }

    fn view(&self, _theme: &Theme) -> rxui::View<ParentAction> {
        component::<ChildCounter, ParentAction>(CounterProps { step: self.step }, |effect| {
            match effect {
                CounterEffect::Changed(value) => ParentAction::ChildChanged(value),
            }
        })
    }
}

fn activate_increment(host: &mut ComponentHost<Parent>) {
    let button = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Increment")
        .unwrap();
    let point = astrelis_core::geometry::LogicalPoint::new(
        button.bounds.origin.x + 5.0,
        button.bounds.origin.y + 5.0,
    );
    host.input(rxui::core::UiInput::PointerPressed(point))
        .unwrap();
    host.input(rxui::core::UiInput::PointerReleased(point))
        .unwrap();
}

#[test]
fn nested_component_preserves_local_state_and_maps_effects() {
    let mut host = ComponentHost::new(
        Parent {
            step: 1,
            child_value: 0,
        },
        LogicalSize::new(400.0, 100.0),
        Theme::dark(),
    )
    .unwrap();

    activate_increment(&mut host);
    assert_eq!(host.component().child_value, 1);

    host.dispatch(ParentAction::SetStep(2)).unwrap();
    activate_increment(&mut host);
    assert_eq!(
        host.component().child_value,
        3,
        "prop updates must not recreate child-local state"
    );
}

#[derive(Clone)]
enum ControlsAction {
    Checked(bool),
    Value(f32),
}

struct Controls {
    checked: bool,
    value: f32,
}

impl Component for Controls {
    type Action = ControlsAction;
    type Effect = ();

    fn update(&mut self, action: ControlsAction, _context: &mut ComponentContext<'_, ()>) {
        match action {
            ControlsAction::Checked(checked) => self.checked = checked,
            ControlsAction::Value(value) => self.value = value,
        }
    }

    fn view(&self, _theme: &Theme) -> rxui::View<ControlsAction> {
        column((
            checkbox("Visible", self.checked, ControlsAction::Checked),
            slider("Opacity", self.value, 0.0..=1.0, ControlsAction::Value),
        ))
    }
}

#[test]
fn controlled_checkbox_and_slider_route_values() {
    let mut host = ComponentHost::new(
        Controls {
            checked: false,
            value: 0.0,
        },
        LogicalSize::new(300.0, 100.0),
        Theme::dark(),
    )
    .unwrap();
    let semantics = host.ui().semantic_snapshot();
    let checkbox = semantics
        .iter()
        .find(|node| node.data.label == "Visible")
        .unwrap();
    let point = astrelis_core::geometry::LogicalPoint::new(
        checkbox.bounds.origin.x + 5.0,
        checkbox.bounds.origin.y + 5.0,
    );
    host.input(rxui::core::UiInput::PointerPressed(point))
        .unwrap();
    host.input(rxui::core::UiInput::PointerReleased(point))
        .unwrap();
    assert!(host.component().checked);

    let slider = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Opacity")
        .unwrap();
    let point = astrelis_core::geometry::LogicalPoint::new(
        slider.bounds.origin.x + slider.bounds.size.width * 0.5,
        slider.bounds.origin.y + slider.bounds.size.height * 0.5,
    );
    host.input(rxui::core::UiInput::PointerPressed(point))
        .unwrap();
    host.input(rxui::core::UiInput::PointerReleased(point))
        .unwrap();
    assert!((host.component().value - 0.5).abs() < 0.01);
}

#[derive(Clone)]
enum EnabledAction {
    Activate,
    SetEnabled(bool),
}

struct EnabledControl {
    enabled: bool,
    activations: usize,
}

impl Component for EnabledControl {
    type Action = EnabledAction;
    type Effect = ();

    fn update(&mut self, action: Self::Action, _context: &mut ComponentContext<'_, ()>) {
        match action {
            EnabledAction::Activate => self.activations += 1,
            EnabledAction::SetEnabled(enabled) => self.enabled = enabled,
        }
    }

    fn view(&self, _theme: &Theme) -> rxui::View<Self::Action> {
        button("Conditional", EnabledAction::Activate).enabled(self.enabled)
    }
}

fn activate_conditional(host: &mut ComponentHost<EnabledControl>) {
    let button = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Conditional")
        .unwrap();
    let point = astrelis_core::geometry::LogicalPoint::new(
        button.bounds.origin.x + 5.0,
        button.bounds.origin.y + 5.0,
    );
    host.input(rxui::core::UiInput::PointerPressed(point))
        .unwrap();
    host.input(rxui::core::UiInput::PointerReleased(point))
        .unwrap();
}

#[test]
fn enabled_modifier_controls_a_retained_subtree_without_recreation() {
    let mut host = ComponentHost::new(
        EnabledControl {
            enabled: false,
            activations: 0,
        },
        LogicalSize::new(300.0, 100.0),
        Theme::dark(),
    )
    .unwrap();
    let before = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Conditional")
        .unwrap();
    assert!(!before.enabled);
    activate_conditional(&mut host);
    assert_eq!(host.component().activations, 0);

    host.dispatch(EnabledAction::SetEnabled(true)).unwrap();
    let after = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Conditional")
        .unwrap();
    assert_eq!(after.id, before.id);
    assert!(after.enabled);
    activate_conditional(&mut host);
    assert_eq!(host.component().activations, 1);
}

#[derive(Clone)]
enum OverlayAction {
    Toggle,
}

struct Overlay {
    open: bool,
}

impl Component for Overlay {
    type Action = OverlayAction;
    type Effect = ();

    fn update(&mut self, action: Self::Action, _context: &mut ComponentContext<'_, ()>) {
        match action {
            OverlayAction::Toggle => self.open = !self.open,
        }
    }

    fn view(&self, _theme: &Theme) -> rxui::View<Self::Action> {
        stack((
            button("Open", OverlayAction::Toggle).enabled(!self.open),
            button("Close", OverlayAction::Toggle)
                .visible(self.open)
                .focus_scope(self.open)
                .dismiss_on_escape(OverlayAction::Toggle),
        ))
    }
}

#[test]
fn focus_scope_autofocuses_restores_and_routes_escape() {
    let mut host = ComponentHost::new(
        Overlay { open: false },
        LogicalSize::new(300.0, 100.0),
        Theme::dark(),
    )
    .unwrap();
    let root = host.ui().root();
    host.ui_mut().focus_first_in_subtree(root).unwrap();
    host.ui_mut().update_passes().unwrap();
    assert!(
        host.ui()
            .semantic_snapshot()
            .into_iter()
            .any(|node| node.data.label == "Open" && node.focused)
    );

    host.dispatch(OverlayAction::Toggle).unwrap();
    assert!(
        host.ui()
            .semantic_snapshot()
            .into_iter()
            .any(|node| node.data.label == "Close" && node.focused)
    );

    host.input(rxui::core::UiInput::Keyboard {
        input: KeyboardInput {
            device_id: DeviceId(1),
            physical_key: PhysicalKey::Unidentified,
            logical_key: Key::Named(NamedKey::Escape),
            text: None,
            location: KeyLocation::Standard,
            state: ElementState::Pressed,
            repeat: false,
            synthetic: false,
        },
        modifiers: Modifiers::default(),
    })
    .unwrap();
    assert!(!host.component().open);
    assert!(
        host.ui()
            .semantic_snapshot()
            .into_iter()
            .any(|node| node.data.label == "Open" && node.focused)
    );
}

#[derive(Clone)]
enum PaletteAction {
    Query(String),
    Previous,
    Next,
    Invoke(u8),
    Dismiss,
}

struct Palette {
    selected: usize,
    invoked: Option<u8>,
}

impl Component for Palette {
    type Action = PaletteAction;
    type Effect = ();

    fn update(&mut self, action: Self::Action, _context: &mut ComponentContext<'_, ()>) {
        match action {
            PaletteAction::Query(query) => {
                let _ = query;
                self.selected = 0;
            }
            PaletteAction::Previous | PaletteAction::Next => {
                self.selected = (self.selected + 1) % 2;
            }
            PaletteAction::Invoke(command) => self.invoked = Some(command),
            PaletteAction::Dismiss => {}
        }
    }

    fn view(&self, _theme: &Theme) -> rxui::View<Self::Action> {
        command_palette(
            true,
            "",
            &[
                CommandItem {
                    id: "one".into(),
                    label: "One".into(),
                    description: None,
                    action: PaletteAction::Invoke(1),
                    enabled: true,
                },
                CommandItem {
                    id: "two".into(),
                    label: "Two".into(),
                    description: None,
                    action: PaletteAction::Invoke(2),
                    enabled: true,
                },
            ],
            self.selected,
            PaletteAction::Query,
            CommandPaletteNavigation {
                dismiss: PaletteAction::Dismiss,
                previous: PaletteAction::Previous,
                next: PaletteAction::Next,
            },
        )
    }
}

#[test]
fn command_palette_navigation_bubbles_through_the_search_field() {
    let mut host = ComponentHost::new(
        Palette {
            selected: 0,
            invoked: None,
        },
        LogicalSize::new(640.0, 480.0),
        Theme::dark(),
    )
    .unwrap();
    for key in [NamedKey::Other("ArrowDown".into()), NamedKey::Enter] {
        host.input(rxui::core::UiInput::Keyboard {
            input: KeyboardInput {
                device_id: DeviceId(1),
                physical_key: PhysicalKey::Unidentified,
                logical_key: Key::Named(key),
                text: None,
                location: KeyLocation::Standard,
                state: ElementState::Pressed,
                repeat: false,
                synthetic: false,
            },
            modifiers: Modifiers::default(),
        })
        .unwrap();
    }
    assert_eq!(host.component().selected, 1);
    assert_eq!(host.component().invoked, Some(2));
}
