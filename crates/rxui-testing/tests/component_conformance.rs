//! Component synthetic scene goldens and interaction traces.

use astrelis_core::geometry::{LogicalPoint, LogicalSize};
use astrelis_platform::{
    CursorIcon, DeviceId, ElementState, Key, KeyLocation, KeyboardInput, Modifiers, NamedKey,
    PhysicalKey,
};
use rxui::core::{Axis, SemanticData, SemanticRole, UiInput};
use rxui::{
    ButtonStyle, ButtonVariant, ColorRole, Component, ComponentContext, ComponentHost,
    ContainerStyle, DialogAction, FrameStyle, Space, Theme, View, button, button_with, dialog,
    label, panel, row_with, split_pane, stack,
};
use rxui_testing::differential::SemanticScene;
use rxui_testing::golden::assert_text_golden;

#[derive(Clone)]
enum SplitAction {
    Resize(f32),
}

struct SplitScene {
    ratio: f32,
}

impl Component for SplitScene {
    type Action = SplitAction;
    type Effect = ();

    fn update(&mut self, action: SplitAction, _context: &mut ComponentContext<'_, ()>) {
        match action {
            SplitAction::Resize(ratio) => self.ratio = ratio,
        }
    }

    fn view(&self, _theme: &Theme) -> View<SplitAction> {
        split_pane(
            Axis::Horizontal,
            self.ratio,
            panel(
                LogicalSize::new(1.0, 1.0),
                rxui::ColorRole::Surface,
                Some(SemanticData {
                    role: SemanticRole::Group,
                    label: "First pane".into(),
                    ..SemanticData::default()
                }),
            ),
            panel(
                LogicalSize::new(1.0, 1.0),
                rxui::ColorRole::Background,
                Some(SemanticData {
                    role: SemanticRole::Group,
                    label: "Second pane".into(),
                    ..SemanticData::default()
                }),
            ),
            SplitAction::Resize,
        )
        .frame(FrameStyle::new().grow(1.0))
    }
}

#[derive(Clone)]
enum ModalAction {
    Background,
    Dismiss,
    Confirm,
}

struct ModalScene {
    open: bool,
    background_activations: usize,
}

impl Component for ModalScene {
    type Action = ModalAction;
    type Effect = ();

    fn update(&mut self, action: ModalAction, _context: &mut ComponentContext<'_, ()>) {
        match action {
            ModalAction::Background => self.background_activations += 1,
            ModalAction::Dismiss | ModalAction::Confirm => self.open = false,
        }
    }

    fn view(&self, _theme: &Theme) -> View<ModalAction> {
        dialog(
            self.open,
            "Synthetic dialog",
            button("Background action", ModalAction::Background),
            label("Dialog content"),
            ModalAction::Dismiss,
            &[
                DialogAction {
                    label: "Cancel".into(),
                    action: ModalAction::Dismiss,
                    variant: ButtonVariant::Quiet,
                },
                DialogAction {
                    label: "Confirm".into(),
                    action: ModalAction::Confirm,
                    variant: ButtonVariant::Primary,
                },
            ],
        )
    }
}

struct HoverScene;

impl Component for HoverScene {
    type Action = ();
    type Effect = ();

    fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> View<()> {
        button("Hover target", ())
    }
}

struct GrowScene;

impl Component for GrowScene {
    type Action = ();
    type Effect = ();

    fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> View<()> {
        let region = |label: &str, grow| {
            panel(
                LogicalSize::new(1.0, 1.0),
                ColorRole::Surface,
                Some(SemanticData {
                    role: SemanticRole::Group,
                    label: label.into(),
                    ..SemanticData::default()
                }),
            )
            .frame(FrameStyle::new().grow(grow))
        };
        row_with(
            ContainerStyle::new().gap(Space::None),
            (
                region("Grow 1a", 1.0),
                region("Grow 2", 2.0),
                region("Grow 1b", 1.0),
            ),
        )
    }
}

#[derive(Clone)]
enum StackAction {
    Back,
    Front,
}

struct StackScene {
    back: usize,
    front: usize,
}

impl Component for StackScene {
    type Action = StackAction;
    type Effect = ();

    fn update(&mut self, action: StackAction, _context: &mut ComponentContext<'_, ()>) {
        match action {
            StackAction::Back => self.back += 1,
            StackAction::Front => self.front += 1,
        }
    }

    fn view(&self, _theme: &Theme) -> View<StackAction> {
        let style = ButtonStyle::standard().size(LogicalSize::new(120.0, 40.0));
        stack((
            button_with("Back layer", StackAction::Back, style),
            button_with("Front layer", StackAction::Front, style),
        ))
    }
}

fn scene<C: Component>(component: C, viewport: LogicalSize) -> SemanticScene {
    let host = ComponentHost::new(component, viewport, Theme::dark()).unwrap();
    SemanticScene::from_component(&host.ui().semantic_snapshot())
}

#[test]
fn synthetic_component_layouts_match_reviewed_golden() {
    let mut snapshot = String::new();
    for ratio in [0.25, 0.5, 0.75] {
        snapshot.push_str(&format!("[split {ratio:.2}]\n"));
        snapshot.push_str(&scene(SplitScene { ratio }, LogicalSize::new(400.0, 240.0)).snapshot());
    }
    snapshot.push_str("[modal 640x480]\n");
    snapshot.push_str(
        &scene(
            ModalScene {
                open: true,
                background_activations: 0,
            },
            LogicalSize::new(640.0, 480.0),
        )
        .snapshot(),
    );
    snapshot.push_str("[grow 1-2-1]\n");
    snapshot.push_str(&scene(GrowScene, LogicalSize::new(400.0, 120.0)).snapshot());
    snapshot.push_str("[stack overlap]\n");
    snapshot.push_str(
        &scene(
            StackScene { back: 0, front: 0 },
            LogicalSize::new(240.0, 100.0),
        )
        .snapshot(),
    );
    snapshot.push_str("[modal 320x240]\n");
    snapshot.push_str(
        &scene(
            ModalScene {
                open: true,
                background_activations: 0,
            },
            LogicalSize::new(320.0, 240.0),
        )
        .snapshot(),
    );
    assert_text_golden(
        &snapshot,
        include_str!("goldens/next-synthetic-scenes.txt"),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/goldens/next-synthetic-scenes.txt"
        ),
    );
}

#[test]
fn splitter_click_drag_capture_and_release_trace_is_stable() {
    let mut host = ComponentHost::new(
        SplitScene { ratio: 0.5 },
        LogicalSize::new(400.0, 240.0),
        Theme::dark(),
    )
    .unwrap();
    let first = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "First pane")
        .unwrap()
        .bounds;
    let divider = LogicalPoint::new(first.origin.x + first.size.width + 3.0, 120.0);

    host.input(UiInput::PointerMoved(divider)).unwrap();
    assert_eq!(host.ui().cursor_icon(), CursorIcon::EwResize);
    host.input(UiInput::PointerPressed(divider)).unwrap();
    host.input(UiInput::PointerReleased(divider)).unwrap();
    assert!((host.component().ratio - 0.5).abs() < 0.001);

    host.input(UiInput::PointerPressed(divider)).unwrap();
    host.input(UiInput::PointerMoved(LogicalPoint::new(300.0, 120.0)))
        .unwrap();
    assert!((0.7..0.8).contains(&host.component().ratio));
    host.input(UiInput::PointerReleased(LogicalPoint::new(450.0, 120.0)))
        .unwrap();
    assert!((host.component().ratio - 0.95).abs() < 0.001);
    host.input(UiInput::PointerMoved(LogicalPoint::new(100.0, 120.0)))
        .unwrap();
    assert!((host.component().ratio - 0.95).abs() < 0.001);
}

#[test]
fn modal_disables_background_autofocuses_and_dismisses_on_escape() {
    let mut host = ComponentHost::new(
        ModalScene {
            open: true,
            background_activations: 0,
        },
        LogicalSize::new(640.0, 480.0),
        Theme::dark(),
    )
    .unwrap();
    let nodes = host.ui().semantic_snapshot();
    let background = nodes
        .iter()
        .find(|node| node.data.label == "Background action")
        .unwrap();
    let cancel = nodes
        .iter()
        .find(|node| node.data.label == "Cancel")
        .unwrap();
    assert!(!background.enabled);
    assert!(cancel.focused);

    host.semantic_action(background.id, rxui::core::SemanticAction::Activate)
        .unwrap();
    assert_eq!(host.component().background_activations, 0);

    host.input(UiInput::Keyboard {
        input: KeyboardInput {
            device_id: DeviceId(1),
            physical_key: PhysicalKey::Unidentified,
            logical_key: Key::Named(NamedKey::Escape),
            text: None,
            location: KeyLocation::Standard,
            state: ElementState::Pressed,
            repeat: false,
            synthetic: true,
        },
        modifiers: Modifiers::default(),
    })
    .unwrap();
    assert!(!host.component().open);
}

#[test]
fn hover_entry_and_window_exit_update_cursor_and_repaint_locally() {
    let mut host =
        ComponentHost::new(HoverScene, LogicalSize::new(240.0, 80.0), Theme::dark()).unwrap();
    let bounds = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Hover target")
        .unwrap()
        .bounds;
    let point = LogicalPoint::new(
        bounds.origin.x + bounds.size.width * 0.5,
        bounds.origin.y + bounds.size.height * 0.5,
    );
    let enter = host
        .input(UiInput::PointerMoved(point))
        .unwrap()
        .unwrap()
        .stats;
    assert_eq!(host.ui().cursor_icon(), CursorIcon::Pointer);
    assert!(enter.rebuilt_fragments <= 2);

    let leave = host.input(UiInput::PointerLeft).unwrap().unwrap().stats;
    assert_eq!(host.ui().cursor_icon(), CursorIcon::Default);
    assert!(leave.rebuilt_fragments <= 2);
}

#[test]
fn proportional_growth_and_stack_hit_order_are_deterministic() {
    let grow = scene(GrowScene, LogicalSize::new(400.0, 120.0));
    let widths = grow
        .landmarks
        .iter()
        .map(|landmark| landmark.bounds.size.width)
        .collect::<Vec<_>>();
    assert_eq!(widths, vec![100.0, 200.0, 100.0]);

    let mut host = ComponentHost::new(
        StackScene { back: 0, front: 0 },
        LogicalSize::new(240.0, 100.0),
        Theme::dark(),
    )
    .unwrap();
    let front = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Front layer")
        .unwrap()
        .bounds;
    let point = LogicalPoint::new(
        front.origin.x + front.size.width * 0.5,
        front.origin.y + front.size.height * 0.5,
    );
    host.input(UiInput::PointerPressed(point)).unwrap();
    host.input(UiInput::PointerReleased(point)).unwrap();
    assert_eq!(host.component().back, 0);
    assert_eq!(host.component().front, 1);
}
