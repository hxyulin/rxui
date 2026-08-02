//! Component synthetic scene goldens and interaction traces.

use rxui::{
    Axis, ButtonStyle, ButtonVariant, ColorRole, Component, ComponentContext, ContainerStyle,
    FrameStyle, Space, Theme, View, button, button_with,
    geometry::{LogicalPoint, LogicalSize},
    input::{CursorIcon, NamedKey},
    label, panel_with_semantics, row_with,
    semantics::{SemanticData, SemanticRole},
    split_pane, stack,
    surfaces::{DialogAction, dialog},
};
use rxui_test_support::{Harness, assert_text_golden};

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
            panel_with_semantics(
                LogicalSize::new(1.0, 1.0),
                rxui::ColorRole::Surface,
                SemanticData {
                    role: SemanticRole::Group,
                    label: "First pane".into(),
                    ..SemanticData::default()
                },
            ),
            panel_with_semantics(
                LogicalSize::new(1.0, 1.0),
                rxui::ColorRole::Background,
                SemanticData {
                    role: SemanticRole::Group,
                    label: "Second pane".into(),
                    ..SemanticData::default()
                },
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

impl ModalScene {
    const fn open() -> Self {
        Self {
            open: true,
            background_activations: 0,
        }
    }
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
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    action: ModalAction::Dismiss,
                    variant: ButtonVariant::Quiet,
                },
                DialogAction {
                    id: "confirm".into(),
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
            panel_with_semantics(
                LogicalSize::new(1.0, 1.0),
                ColorRole::Surface,
                SemanticData {
                    role: SemanticRole::Group,
                    label: label.into(),
                    ..SemanticData::default()
                },
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

fn mount<C: Component>(component: C, viewport: LogicalSize) -> Harness<C> {
    Harness::new(component, viewport).expect("synthetic scene mounts")
}

#[test]
fn synthetic_component_layouts_match_reviewed_golden() {
    let mut snapshot = String::new();
    for ratio in [0.25, 0.5, 0.75] {
        snapshot.push_str(&format!("[split {ratio:.2}]\n"));
        snapshot.push_str(&mount(SplitScene { ratio }, LogicalSize::new(400.0, 240.0)).snapshot());
    }
    snapshot.push_str("[modal 640x480]\n");
    snapshot.push_str(&mount(ModalScene::open(), LogicalSize::new(640.0, 480.0)).snapshot());
    snapshot.push_str("[grow 1-2-1]\n");
    snapshot.push_str(&mount(GrowScene, LogicalSize::new(400.0, 120.0)).snapshot());
    snapshot.push_str("[stack overlap]\n");
    snapshot.push_str(
        &mount(
            StackScene { back: 0, front: 0 },
            LogicalSize::new(240.0, 100.0),
        )
        .snapshot(),
    );
    snapshot.push_str("[modal 320x240]\n");
    snapshot.push_str(&mount(ModalScene::open(), LogicalSize::new(320.0, 240.0)).snapshot());
    assert_text_golden(
        &snapshot,
        include_str!("goldens/synthetic-scenes.txt"),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/goldens/synthetic-scenes.txt"
        ),
    );
}

#[test]
fn splitter_click_drag_capture_and_release_trace_is_stable() {
    let mut harness = mount(SplitScene { ratio: 0.5 }, LogicalSize::new(400.0, 240.0));
    // The splitter handle publishes no accessible node of its own, so it has to
    // be located from the pane whose trailing edge it follows.
    let first = harness.bounds("First pane");
    let divider = LogicalPoint::new(first.origin.x + first.size.width + 3.0, 120.0);

    harness.hover_at(divider);
    assert_eq!(harness.cursor_icon(), CursorIcon::EwResize);
    harness.click_at(divider);
    assert!((harness.component().ratio - 0.5).abs() < 0.001);

    harness.press_pointer_at(divider);
    harness.hover_at(LogicalPoint::new(300.0, 120.0));
    assert!((0.7..0.8).contains(&harness.component().ratio));
    harness.release_pointer_at(LogicalPoint::new(450.0, 120.0));
    assert!((harness.component().ratio - 0.95).abs() < 0.001);
    harness.hover_at(LogicalPoint::new(100.0, 120.0));
    assert!((harness.component().ratio - 0.95).abs() < 0.001);
}

#[test]
fn modal_disables_background_autofocuses_and_dismisses_on_escape() {
    let mut harness = mount(ModalScene::open(), LogicalSize::new(640.0, 480.0));
    assert!(!harness.find("Background action").enabled);
    assert!(harness.find("Cancel").focused);

    harness.activate("Background action");
    assert_eq!(harness.component().background_activations, 0);

    harness.press(NamedKey::Escape);
    assert!(!harness.component().open);
}

#[test]
fn hover_entry_and_window_exit_update_cursor_and_repaint_locally() {
    let mut harness = mount(HoverScene, LogicalSize::new(240.0, 80.0));

    harness.hover("Hover target");
    assert_eq!(harness.cursor_icon(), CursorIcon::Pointer);
    assert!(harness.stats().rebuilt_fragments <= 2);

    harness.pointer_left();
    assert_eq!(harness.cursor_icon(), CursorIcon::Default);
    assert!(harness.stats().rebuilt_fragments <= 2);
}

#[test]
fn proportional_growth_and_stack_hit_order_are_deterministic() {
    let grow = mount(GrowScene, LogicalSize::new(400.0, 120.0));
    let widths = grow
        .scene()
        .landmarks
        .iter()
        .map(|landmark| landmark.bounds.size.width)
        .collect::<Vec<_>>();
    assert_eq!(widths, vec![100.0, 200.0, 100.0]);

    let mut stack = mount(
        StackScene { back: 0, front: 0 },
        LogicalSize::new(240.0, 100.0),
    );
    stack.click("Front layer");
    assert_eq!(stack.component().back, 0);
    assert_eq!(stack.component().front, 1);
}
