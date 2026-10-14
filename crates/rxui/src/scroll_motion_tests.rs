//! Smooth scrolling driven by explicit frame times.
//!
//! Every scenario shares one view type: each `View` type instantiates all of
//! `Ui<T>`, and the debug test binary is close to Mach-O object size limits.
use crate::*;
use std::time::Duration;

fn ms(v: u64) -> Duration {
    Duration::from_millis(v)
}
/// Eased progress of the 150 ms theme scroll duration after `elapsed` ms.
fn eased(elapsed: u64) -> f32 {
    Easing::EaseOut.apply(elapsed as f32 / 150.)
}
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}
/// 10 units per character and 20 per line, with carets, for text area reveal.
struct Lines;
impl TextMeasure for Lines {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        let width = r.text.split('\n').map(str::len).max().unwrap_or(0);
        Ok([width as f32 * 10., r.text.split('\n').count() as f32 * 20.])
    }
    fn supports_text_geometry(&self) -> bool {
        true
    }
    fn text_caret(
        &mut self,
        _: ElementId,
        r: TextRequest<'_>,
        p: TextPosition,
    ) -> Result<Option<Bounds>, UiError> {
        let before = &r.text[..p.byte_offset];
        let line = before.matches('\n').count();
        let column = before.len() - before.rfind('\n').map_or(0, |i| i + 1);
        Ok(Some(Bounds {
            x: column as f32 * 10.,
            y: line as f32 * 20.,
            width: 1.,
            height: 20.,
        }))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Scene {
    /// A 100-unit scroll viewport over a 20-line (400-unit) text area.
    Feed,
    /// A scroll container whose wheel listener records sources and prevents steps.
    Canvas,
    /// A 1,000-row fixed-height list whose rows reveal row `reveal`.
    List,
    /// A variable-height list over `rows` whose rows scroll by 200.
    Rows,
}
struct Harness {
    scene: Scene,
    handle: ScrollHandle,
    placement: Option<ScrollPlacement>,
    behavior: ScrollBehavior,
    text: String,
    sources: Vec<WheelSource>,
    reveal: usize,
    rows: VirtualRows,
}
impl View for Harness {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        match self.scene {
            Scene::Feed => column()
                .child(
                    button("Capture")
                        .key("capture")
                        .on_click(cx.listener(|this, _, cx| {
                            this.placement = Some(this.handle.placement(cx).unwrap());
                        })),
                )
                .child(
                    column()
                        .key("scroll")
                        .height(100.)
                        .scroll_y()
                        .scroll_behavior(self.behavior)
                        .scroll_handle(self.handle.clone())
                        .child(
                            text_area(self.text.clone())
                                .key("area")
                                .width(300.)
                                .padding(0.)
                                .border(0., ThemeColor::InputBorder)
                                .min_height(0.)
                                .on_change(cx.listener(|this, e: &TextChangeEvent, _| {
                                    this.text = e.value.clone();
                                })),
                        ),
                ),
            Scene::Canvas => column()
                .key("scroll")
                .size(100., 100.)
                .scroll_y()
                .scroll_behavior(self.behavior)
                .scroll_handle(self.handle.clone())
                .on_wheel(cx.listener(|this, e: &WheelInput, _| {
                    this.sources.push(e.source);
                    if e.source == WheelSource::Lines {
                        e.prevent_default();
                    }
                }))
                .child(column().size(100., 500.)),
            Scene::List => virtual_list(1000, 20., &self.handle, cx, |index| {
                button(format!("row{index}"))
                    .key(index)
                    .padding(0.)
                    .on_click(cx.listener(|this, _, cx| {
                        this.handle.reveal_row(cx, this.reveal, 20.).unwrap();
                    }))
            })
            .scroll_behavior(self.behavior)
            .fill_width()
            .fill_height()
            .into_element(),
            Scene::Rows => variable_list(&self.rows, &self.handle, cx, |index| {
                button(format!("item{index}"))
                    .padding(0.)
                    .on_click(cx.listener(|this, _, cx| {
                        this.handle.scroll_by(cx, [0., 200.]).unwrap();
                    }))
            })
            .scroll_behavior(self.behavior)
            .fill_width()
            .fill_height()
            .into_element(),
        }
    }
}
fn harness(
    scene: Scene,
    behavior: ScrollBehavior,
    rows: VirtualRows,
) -> (Runtime, Entity<Harness>, Ui<Harness>) {
    let mut r = Runtime::new();
    let text = (0..20).map(|i| format!("line {i}")).collect::<Vec<_>>();
    let root = r.update(|cx| {
        cx.new(|_| Harness {
            scene,
            handle: ScrollHandle::new(),
            placement: None,
            behavior,
            text: text.join("\n"),
            sources: Vec::new(),
            reveal: 500,
            rows,
        })
    });
    let mut ui = Ui::new(&mut r, root.clone()).unwrap();
    ui.prepare(&mut r, [400., 400.], &mut Lines).unwrap();
    (r, root, ui)
}
/// The text area feed with its scroll placement captured by a click.
fn feed(behavior: ScrollBehavior) -> (Runtime, Entity<Harness>, Ui<Harness>) {
    let (mut r, root, mut ui) = harness(Scene::Feed, behavior, VirtualRows::new());
    let b = ui.element(keyed(&ui, "capture")).unwrap().bounds;
    click(&mut r, &mut ui, [b.x + 5., b.y + 5.]);
    ui.prepare(&mut r, [400., 400.], &mut Lines).unwrap();
    (r, root, ui)
}
fn keyed(ui: &Ui<Harness>, key: &str) -> ElementId {
    let key = Key::from(key);
    ui.elements().find(|e| e.key == Some(&key)).unwrap().id
}
fn frame(r: &mut Runtime, ui: &mut Ui<Harness>, time: u64) {
    r.begin_frame(ms(time));
    ui.prepare(r, [400., 400.], &mut Lines).unwrap();
}
fn click(r: &mut Runtime, ui: &mut Ui<Harness>, point: [f32; 2]) {
    ui.pointer(r, PointerEvent::Pressed(point)).unwrap();
    ui.pointer(r, PointerEvent::Released(point)).unwrap();
}
fn offset(ui: &Ui<Harness>) -> f32 {
    ui.element(keyed(ui, "scroll")).unwrap().scroll_offset[1]
}
fn command(
    r: &mut Runtime,
    root: &Entity<Harness>,
    f: impl FnOnce(&ScrollPlacement, &mut AppContext<'_>) -> Result<(), ScrollError>,
) {
    r.update(|cx| {
        let placement = root.read(cx).placement.clone().unwrap();
        f(&placement, cx).unwrap();
    });
}
fn lines(r: &mut Runtime, ui: &mut Ui<Harness>, dy: f32) -> InputResult {
    let b = ui.element(keyed(ui, "scroll")).unwrap().bounds;
    ui.wheel_with_source(
        r,
        [b.x + 10., b.y + 10.],
        [0., dy],
        Modifiers::default(),
        WheelSource::Lines,
    )
    .unwrap()
}
fn precise(r: &mut Runtime, ui: &mut Ui<Harness>, dy: f32) {
    let b = ui.element(keyed(ui, "scroll")).unwrap().bounds;
    assert!(
        ui.wheel(r, [b.x + 10., b.y + 10.], [0., dy], Modifiers::default())
            .unwrap()
            .changed
    );
}
fn list_offset(ui: &Ui<Harness>) -> f32 {
    ui.semantics()
        .find(|n| n.scroll_range[1] > 0.)
        .unwrap()
        .scroll_offset[1]
}
fn row_top(ui: &Ui<Harness>, name: &str) -> Option<f32> {
    let id = ui
        .semantics()
        .find(|n| n.role == SemanticRole::Button && n.label == Some(name))?
        .id;
    ui.elements().find(|n| n.id == id).map(|n| n.bounds.y)
}

#[test]
fn programmatic_scrolls_ease_over_frames_and_then_stop_requesting_them() {
    let (mut r, root, mut ui) = feed(ScrollBehavior::Smooth);
    frame(&mut r, &mut ui, 1000);
    command(&mut r, &root, |p, cx| p.scroll_to(cx, [0., 200.]));
    // Applied in a fresh frame, the move starts there at its starting offset.
    frame(&mut r, &mut ui, 1016);
    assert_eq!(offset(&ui), 0.);
    assert!(ui.is_animating());
    assert!(r.animation_frame_requested());
    frame(&mut r, &mut ui, 1091);
    assert!(close(offset(&ui), 200. * eased(75)));
    assert!(offset(&ui) > 100.);
    // Scroll metrics follow the motion.
    let state = r.update(|cx| root.read(cx).placement.clone().unwrap().state().unwrap());
    assert_eq!(state.offset[1], offset(&ui));
    frame(&mut r, &mut ui, 1166);
    assert_eq!(offset(&ui), 200.);
    assert!(!ui.is_animating());
    assert!(!r.animation_frame_requested());
    // Relative motion and reveals animate as well; scroll_by adds to the target.
    command(&mut r, &root, |p, cx| p.scroll_by(cx, [0., 50.]));
    command(&mut r, &root, |p, cx| p.scroll_by(cx, [0., 50.]));
    frame(&mut r, &mut ui, 1200);
    frame(&mut r, &mut ui, 1400);
    assert_eq!(offset(&ui), 300.);
    command(&mut r, &root, |p, cx| p.reveal_span(cx, 20., 20.));
    frame(&mut r, &mut ui, 1500);
    frame(&mut r, &mut ui, 1575);
    assert!(close(offset(&ui), 300. - 280. * eased(75)));
    frame(&mut r, &mut ui, 1650);
    assert_eq!(offset(&ui), 20.);
}
#[test]
fn wheel_steps_animate_and_accumulate_while_precise_deltas_move_directly() {
    let (mut r, _, mut ui) = feed(ScrollBehavior::Smooth);
    // Hover the text area first, so no hover transition is in flight later.
    let b = ui.element(keyed(&ui, "scroll")).unwrap().bounds;
    ui.pointer(&mut r, PointerEvent::Moved([b.x + 10., b.y + 10.]))
        .unwrap();
    frame(&mut r, &mut ui, 500);
    frame(&mut r, &mut ui, 1000);
    // Two quick wheel steps between frames head for their combined target.
    assert!(lines(&mut r, &mut ui, 40.).changed);
    assert!(lines(&mut r, &mut ui, 40.).changed);
    assert_eq!(offset(&ui), 0.);
    assert!(ui.is_animating());
    frame(&mut r, &mut ui, 1016);
    assert_eq!(offset(&ui), 0.);
    frame(&mut r, &mut ui, 1091);
    assert!(close(offset(&ui), 80. * eased(75)));
    frame(&mut r, &mut ui, 1200);
    assert_eq!(offset(&ui), 80.);
    assert!(!ui.is_animating());

    // Trackpad deltas are already smooth: they move at once and start nothing.
    precise(&mut r, &mut ui, 10.);
    assert_eq!(offset(&ui), 90.);
    assert!(!ui.is_animating());

    // A trackpad delta during a smooth scroll ends it at the trackpad's offset.
    lines(&mut r, &mut ui, 40.);
    frame(&mut r, &mut ui, 1216);
    frame(&mut r, &mut ui, 1291);
    let mid = offset(&ui);
    assert!(close(mid, 90. + 40. * eased(75)));
    precise(&mut r, &mut ui, 5.);
    assert_eq!(offset(&ui), mid + 5.);
    frame(&mut r, &mut ui, 1300);
    assert_eq!(offset(&ui), mid + 5.);
    assert!(!ui.is_animating());

    // Wheel steps past the end stop at the range.
    lines(&mut r, &mut ui, 1000.);
    frame(&mut r, &mut ui, 1400);
    frame(&mut r, &mut ui, 1600);
    assert_eq!(offset(&ui), 300.);
}
#[test]
fn listeners_see_the_wheel_source_and_can_prevent_smooth_scrolling() {
    let (mut r, root, mut ui) = harness(Scene::Canvas, ScrollBehavior::Smooth, VirtualRows::new());
    frame(&mut r, &mut ui, 1000);
    let m = Modifiers::default();
    ui.wheel_with_source(&mut r, [50., 50.], [0., 40.], m, WheelSource::Lines)
        .unwrap();
    assert!(!ui.is_animating());
    ui.wheel(&mut r, [50., 50.], [0., 10.], m).unwrap();
    assert_eq!(offset(&ui), 10.);
    r.update(|cx| {
        assert_eq!(
            root.read(cx).sources,
            [WheelSource::Lines, WheelSource::Precise]
        )
    });
}
#[test]
fn reduce_motion_and_hosts_without_a_frame_clock_scroll_immediately() {
    let (mut r, root, mut ui) = feed(ScrollBehavior::Smooth);
    // No frame clock yet: commands apply at once.
    command(&mut r, &root, |p, cx| p.scroll_to(cx, [0., 200.]));
    ui.prepare(&mut r, [400., 400.], &mut Lines).unwrap();
    assert_eq!(offset(&ui), 200.);
    assert!(!ui.is_animating());

    frame(&mut r, &mut ui, 1000);
    r.set_reduce_motion(true);
    frame(&mut r, &mut ui, 1016);
    command(&mut r, &root, |p, cx| p.scroll_to(cx, [0., 0.]));
    frame(&mut r, &mut ui, 1032);
    assert_eq!(offset(&ui), 0.);
    lines(&mut r, &mut ui, 40.);
    assert_eq!(offset(&ui), 40.);
    assert!(!ui.is_animating());

    // Turning reduce motion on mid-flight snaps to the target at the next frame.
    r.set_reduce_motion(false);
    frame(&mut r, &mut ui, 1048);
    command(&mut r, &root, |p, cx| p.scroll_to(cx, [0., 240.]));
    frame(&mut r, &mut ui, 1064);
    frame(&mut r, &mut ui, 1100);
    assert!(offset(&ui) > 40. && offset(&ui) < 240.);
    r.set_reduce_motion(true);
    frame(&mut r, &mut ui, 1116);
    assert_eq!(offset(&ui), 240.);
    assert!(!ui.is_animating());
    assert!(!r.animation_frame_requested());
}
#[test]
fn instant_containers_and_jump_to_skip_the_animation() {
    let (mut r, root, mut ui) = feed(ScrollBehavior::Instant);
    frame(&mut r, &mut ui, 1000);
    command(&mut r, &root, |p, cx| p.scroll_to(cx, [0., 200.]));
    frame(&mut r, &mut ui, 1016);
    assert_eq!(offset(&ui), 200.);
    lines(&mut r, &mut ui, 40.);
    assert_eq!(offset(&ui), 240.);
    assert!(!ui.is_animating());

    let (mut r, root, mut ui) = feed(ScrollBehavior::Smooth);
    frame(&mut r, &mut ui, 1000);
    command(&mut r, &root, |p, cx| p.scroll_to(cx, [0., 300.]));
    frame(&mut r, &mut ui, 1016);
    frame(&mut r, &mut ui, 1050);
    command(&mut r, &root, |p, cx| p.jump_to(cx, [0., 100.]));
    frame(&mut r, &mut ui, 1066);
    assert_eq!(offset(&ui), 100.);
    assert!(!ui.is_animating());
    frame(&mut r, &mut ui, 1300);
    assert_eq!(offset(&ui), 100.);
}
#[test]
fn caret_reveal_during_a_smooth_scroll_wins() {
    let (mut r, root, mut ui) = feed(ScrollBehavior::Smooth);
    frame(&mut r, &mut ui, 1000);
    assert!(ui.focus(keyed(&ui, "area")));
    ui.text_input(
        &mut r,
        TextInputEvent::Move {
            movement: TextMovement::Start,
            extend: false,
        },
        &mut Lines,
    )
    .unwrap();
    frame(&mut r, &mut ui, 1016);
    assert_eq!(offset(&ui), 0.);
    command(&mut r, &root, |p, cx| p.scroll_to(cx, [0., 300.]));
    frame(&mut r, &mut ui, 1032);
    frame(&mut r, &mut ui, 1107);
    assert!(offset(&ui) > 100.);
    // Typing at the caret on line 0 reveals it, ending the scroll there.
    ui.text_input(&mut r, TextInputEvent::Insert("x".into()), &mut Lines)
        .unwrap();
    frame(&mut r, &mut ui, 1123);
    assert_eq!(offset(&ui), 0.);
    frame(&mut r, &mut ui, 1300);
    assert_eq!(offset(&ui), 0.);
    assert!(!ui.is_animating());
    assert!(r.update(|cx| root.read(cx).text.starts_with("xline 0")));
}
#[test]
fn virtual_list_rows_mount_as_a_smooth_reveal_proceeds() {
    let (mut r, _, mut ui) = harness(Scene::List, ScrollBehavior::Smooth, VirtualRows::new());
    frame(&mut r, &mut ui, 1000);
    click(&mut r, &mut ui, [10., 1.]);
    frame(&mut r, &mut ui, 1016);
    assert_eq!(list_offset(&ui), 0.);
    assert!(row_top(&ui, "row0").is_some());
    // Row 500 spans 10_000..10_020; revealing it at the bottom of the 400-unit
    // viewport ends at 9_620.
    for time in [1040, 1091, 1130] {
        frame(&mut r, &mut ui, time);
        let y = list_offset(&ui);
        assert!(close(y, 9_620. * eased(time - 1016)));
        let first = (y / 20.).floor() as usize;
        let name = format!("row{first}");
        assert_eq!(row_top(&ui, &name), Some(first as f32 * 20. - y));
        assert!(ui.elements().count() < 80);
    }
    frame(&mut r, &mut ui, 1200);
    assert_eq!(list_offset(&ui), 9_620.);
    assert_eq!(row_top(&ui, "row500"), Some(380.));
    assert!(row_top(&ui, "row0").is_none());
}
#[test]
fn anchoring_moves_a_smooth_scroll_in_flight_with_the_content() {
    let rows = (0..1000).map(|i| (i, 20.)).collect();
    let (mut r, root, mut ui) = harness(Scene::Rows, ScrollBehavior::Smooth, rows);
    frame(&mut r, &mut ui, 1000);
    let target = ui.semantics().find(|n| n.scroll_range[1] > 0.).unwrap().id;
    ui.semantic_action(
        &mut r,
        SemanticAction::Scroll {
            target,
            offset: [0., 1000.],
        },
        &mut Lines,
    )
    .unwrap();
    frame(&mut r, &mut ui, 1016);
    click(&mut r, &mut ui, [10., 1.]);
    frame(&mut r, &mut ui, 1032);
    assert_eq!(list_offset(&ui), 1000.);
    frame(&mut r, &mut ui, 1091);
    // Three 15-unit rows arrive above the viewport mid-flight.
    r.update(|cx| {
        root.update(cx, |s, _| {
            s.rows.splice(0..0, (1000..1003).map(|k| (k, 15.)))
        })
    });
    frame(&mut r, &mut ui, 1100);
    assert!(close(list_offset(&ui), 1045. + 200. * eased(68)));
    assert!(ui.is_animating());
    frame(&mut r, &mut ui, 1200);
    assert_eq!(list_offset(&ui), 1245.);
    assert!(!ui.is_animating());
}
#[test]
fn platform_behavior_follows_desktop_conventions() {
    let expected = if cfg!(any(target_os = "macos", target_os = "windows")) {
        ScrollBehavior::Smooth
    } else {
        ScrollBehavior::Instant
    };
    assert_eq!(ScrollBehavior::platform(), expected);
    assert_eq!(ScrollBehavior::default(), expected);
    assert_eq!(column().scroll_behavior, expected);
}
