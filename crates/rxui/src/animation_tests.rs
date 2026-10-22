use crate::*;
use std::{cell::Cell, rc::Rc, time::Duration};

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 7., 18.])
    }
}
fn ms(v: u64) -> Duration {
    Duration::from_millis(v)
}
/// Starts the frame at `time` and prepares, as a host does for one redraw.
fn frame<T: View>(r: &mut Runtime, ui: &mut Ui<T>, time: u64) {
    r.begin_frame(ms(time));
    ui.prepare(r, [400., 300.], &mut Measure).unwrap();
}
fn find<T: View>(ui: &Ui<T>, key: &str) -> ElementId {
    ui.elements()
        .find(|e| e.key == Some(&Key::from(key)))
        .unwrap()
        .id
}
fn background<T: View>(ui: &Ui<T>, key: &str) -> Color {
    ui.element(find(ui, key)).unwrap().paint.background.unwrap()
}
fn center<T: View>(ui: &Ui<T>, key: &str) -> [f32; 2] {
    let b = ui.element(find(ui, key)).unwrap().bounds;
    [b.x + b.width / 2., b.y + b.height / 2.]
}
fn close(a: Color, b: Color) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
}
const RED: Color = [1., 0., 0., 1.];
const BLUE: Color = [0., 0., 1., 1.];

#[derive(Default)]
struct Buttons {
    transition: Option<Transition>,
    disabled: bool,
    theme_background: bool,
}
impl View for Buttons {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut target = button("Target")
            .key("target")
            .disabled(self.disabled)
            .background(RED)
            .hover_style(PaintStyle::new().background(BLUE))
            .disabled_style(PaintStyle::new().background(BLUE));
        if let Some(transition) = self.transition {
            target = target.transition(transition);
        }
        let mut page = column().key("page").size(400., 300.).child(target);
        if self.theme_background {
            // A row clipped out of view, like one scrolled away.
            page = page.background(ThemeColor::Surface).child(
                stack().size(40., 10.).clip().child(
                    column()
                        .key("clipped")
                        .absolute()
                        .top(20.)
                        .size(40., 10.)
                        .background(ThemeColor::Surface),
                ),
            );
        }
        page
    }
}
fn buttons(view: Buttons) -> (Runtime, Entity<Buttons>, Ui<Buttons>) {
    let mut r = Runtime::new();
    let root = r.update(|cx| cx.new(|_| view));
    let mut ui = Ui::new(&mut r, root.clone()).unwrap();
    frame(&mut r, &mut ui, 1000);
    (r, root, ui)
}
fn linear() -> Option<Transition> {
    Some(Transition::linear(ms(100)))
}

#[test]
fn hover_transitions_interpolate_settle_and_stop_requesting_frames() {
    let (mut r, _, mut ui) = buttons(Buttons {
        transition: linear(),
        ..Default::default()
    });
    assert!(!ui.is_animating() && !r.animation_frame_requested());
    let point = center(&ui, "target");
    ui.pointer(&mut r, PointerEvent::Moved(point)).unwrap();
    // The move starts at the next frame, from the previous appearance.
    frame(&mut r, &mut ui, 1016);
    assert!(ui.element(find(&ui, "target")).unwrap().hovered);
    assert_eq!(background(&ui, "target"), RED);
    assert!(ui.is_animating() && r.animation_frame_requested());
    frame(&mut r, &mut ui, 1066);
    assert!(close(background(&ui, "target"), [0.5, 0., 0.5, 1.]));
    frame(&mut r, &mut ui, 1116);
    assert_eq!(background(&ui, "target"), BLUE);
    assert!(!ui.is_animating() && !r.animation_frame_requested());
    // Settled: later frames do no preparation work and request nothing.
    let stats = ui.stats();
    for time in [1132, 1148, 5000] {
        frame(&mut r, &mut ui, time);
        assert!(!ui.needs_prepare(&r).unwrap());
        assert!(!r.animation_frame_requested());
    }
    assert_eq!(ui.stats(), stats);
}
#[test]
fn retargeting_mid_flight_continues_from_the_displayed_paint() {
    let (mut r, _, mut ui) = buttons(Buttons {
        transition: linear(),
        ..Default::default()
    });
    let point = center(&ui, "target");
    ui.pointer(&mut r, PointerEvent::Moved(point)).unwrap();
    frame(&mut r, &mut ui, 1000);
    frame(&mut r, &mut ui, 1040);
    let before = background(&ui, "target");
    assert!(close(before, [0.6, 0., 0.4, 1.]));
    ui.pointer(&mut r, PointerEvent::Left).unwrap();
    // No jump at the retarget frame: the new move starts where the old one is.
    frame(&mut r, &mut ui, 1060);
    let at = background(&ui, "target");
    assert!(close(at, [0.4, 0., 0.6, 1.]), "{at:?}");
    frame(&mut r, &mut ui, 1110);
    assert!(close(background(&ui, "target"), [0.7, 0., 0.3, 1.]));
    frame(&mut r, &mut ui, 1160);
    assert_eq!(background(&ui, "target"), RED);
    assert!(!ui.is_animating());
}
#[test]
fn controls_default_to_the_theme_duration_and_no_transition_snaps() {
    let (mut r, root, mut ui) = buttons(Buttons::default());
    let state = Theme::default().sizes().motion.state.as_millis() as u64;
    let point = center(&ui, "target");
    ui.pointer(&mut r, PointerEvent::Moved(point)).unwrap();
    frame(&mut r, &mut ui, 2000);
    frame(&mut r, &mut ui, 2000 + state / 2);
    let mid = background(&ui, "target");
    assert!(mid != RED && mid != BLUE);
    frame(&mut r, &mut ui, 2000 + state);
    assert_eq!(background(&ui, "target"), BLUE);
    assert!(!ui.is_animating());

    r.update(|cx| root.update(cx, |v, _| v.transition = Some(Transition::None)));
    frame(&mut r, &mut ui, 3000);
    ui.pointer(&mut r, PointerEvent::Left).unwrap();
    frame(&mut r, &mut ui, 3016);
    assert_eq!(background(&ui, "target"), RED);
    assert!(!ui.is_animating());
}
#[test]
fn disabled_changes_animate_with_the_description() {
    let (mut r, root, mut ui) = buttons(Buttons {
        transition: linear(),
        ..Default::default()
    });
    r.update(|cx| root.update(cx, |v, _| v.disabled = true));
    frame(&mut r, &mut ui, 1016);
    assert!(ui.element(find(&ui, "target")).unwrap().disabled);
    assert_eq!(background(&ui, "target"), RED);
    frame(&mut r, &mut ui, 1066);
    assert!(close(background(&ui, "target"), [0.5, 0., 0.5, 1.]));
    frame(&mut r, &mut ui, 1116);
    assert_eq!(background(&ui, "target"), BLUE);
}
#[test]
fn reduce_motion_and_hosts_without_a_frame_clock_snap() {
    // Without Runtime::begin_frame, transitions complete immediately.
    let mut r = Runtime::new();
    let root = r.update(|cx| {
        cx.new(|_| Buttons {
            transition: linear(),
            ..Default::default()
        })
    });
    let mut ui = Ui::new(&mut r, root).unwrap();
    ui.prepare(&mut r, [400., 300.], &mut Measure).unwrap();
    let point = center(&ui, "target");
    ui.pointer(&mut r, PointerEvent::Moved(point)).unwrap();
    ui.prepare(&mut r, [400., 300.], &mut Measure).unwrap();
    assert_eq!(background(&ui, "target"), BLUE);
    assert!(!ui.is_animating() && !r.animation_frame_requested());

    // Turning reduce motion on completes a transition in flight at the next frame.
    let (mut r, _, mut ui) = buttons(Buttons {
        transition: linear(),
        ..Default::default()
    });
    ui.pointer(&mut r, PointerEvent::Moved(point)).unwrap();
    frame(&mut r, &mut ui, 1016);
    frame(&mut r, &mut ui, 1040);
    assert!(ui.is_animating());
    r.set_reduce_motion(true);
    assert!(r.reduce_motion());
    frame(&mut r, &mut ui, 1041);
    assert_eq!(background(&ui, "target"), BLUE);
    assert!(!ui.is_animating() && !r.animation_frame_requested());
    ui.pointer(&mut r, PointerEvent::Left).unwrap();
    frame(&mut r, &mut ui, 1057);
    assert_eq!(background(&ui, "target"), RED);
    assert!(!ui.is_animating());
}
#[test]
fn theme_switches_crossfade_plain_containers_and_controls() {
    let (mut r, _, mut ui) = buttons(Buttons {
        theme_background: true,
        ..Default::default()
    });
    let (dark, light) = (Theme::dark(), Theme::light());
    assert_eq!(background(&ui, "page"), dark.palette().surface);
    let text = |ui: &Ui<Buttons>| ui.element(find(ui, "target")).unwrap().color;
    assert!(ui.set_theme(light.clone()).unwrap());
    frame(&mut r, &mut ui, 2000);
    assert_eq!(background(&ui, "page"), dark.palette().surface);
    assert_eq!(text(&ui), dark.palette().text);
    // Elements that were not painted change without a transition.
    assert_eq!(background(&ui, "clipped"), light.palette().surface);
    let state = dark.sizes().motion.state.as_millis() as u64;
    frame(&mut r, &mut ui, 2000 + state / 2);
    let mid = background(&ui, "page");
    assert!(mid != dark.palette().surface && mid != light.palette().surface);
    frame(&mut r, &mut ui, 2000 + state);
    assert_eq!(background(&ui, "page"), light.palette().surface);
    assert_eq!(text(&ui), light.palette().text);
    assert!(!ui.is_animating());
}
#[test]
fn focus_rings_fade_in_and_disappear_immediately() {
    let (mut r, _, mut ui) = buttons(Buttons {
        transition: linear(),
        ..Default::default()
    });
    let id = find(&ui, "target");
    assert!(ui.focus(id));
    frame(&mut r, &mut ui, 1016);
    let focus = |ui: &Ui<Buttons>| ui.element(id).unwrap().paint.focus_color[3];
    assert!(ui.element(id).unwrap().focused);
    assert_eq!(focus(&ui), 0.);
    frame(&mut r, &mut ui, 1066);
    assert!((focus(&ui) - 0.5).abs() < 1e-4);
    frame(&mut r, &mut ui, 1116);
    assert_eq!(focus(&ui), 1.);
    assert!(!ui.is_animating());
}

#[test]
fn focus_rings_fade_in_when_keyboard_input_reveals_pointer_focus() {
    let (mut r, _, mut ui) = buttons(Buttons {
        transition: linear(),
        ..Default::default()
    });
    let id = find(&ui, "target");
    let point = center(&ui, "target");
    ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
    ui.pointer(&mut r, PointerEvent::Released(point)).unwrap();
    // Let the press transition settle; a hidden ring starts no motion of its own.
    frame(&mut r, &mut ui, 1016);
    frame(&mut r, &mut ui, 1200);
    let info = ui.element(id).unwrap();
    assert!(info.focused && !info.focus_visible);
    assert!(!ui.is_animating());

    let key = KeyEvent {
        physical_key: Default::default(),
        key: KeyboardKey::ArrowDown,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    };
    ui.key(&mut r, key).unwrap();
    frame(&mut r, &mut ui, 1216);
    let focus = |ui: &Ui<Buttons>| ui.element(id).unwrap().paint.focus_color[3];
    assert!(ui.element(id).unwrap().focus_visible);
    assert_eq!(focus(&ui), 0.);
    frame(&mut r, &mut ui, 1266);
    assert!((focus(&ui) - 0.5).abs() < 1e-4);
    frame(&mut r, &mut ui, 1316);
    assert_eq!(focus(&ui), 1.);
    assert!(!ui.is_animating());
}

struct Drawer {
    open: bool,
    evaluations: Rc<Cell<u32>>,
}
impl View for Drawer {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        self.evaluations.set(self.evaluations.get() + 1);
        let width = cx.animate(
            "width",
            if self.open { 200. } else { 100. },
            Transition::linear(ms(100)),
        );
        column().key("drawer").width(width).height(20.)
    }
}
#[test]
fn animated_values_evaluate_only_while_moving() {
    let evaluations = Rc::new(Cell::new(0));
    let mut r = Runtime::new();
    let root = r.update(|cx| {
        cx.new(|_| Drawer {
            open: false,
            evaluations: evaluations.clone(),
        })
    });
    let mut ui = Ui::new(&mut r, root.clone()).unwrap();
    frame(&mut r, &mut ui, 1000);
    let width = |ui: &Ui<Drawer>| ui.element(find(ui, "drawer")).unwrap().bounds.width;
    // The first evaluation starts at its target.
    assert_eq!(width(&ui), 100.);
    assert!(!r.animation_frame_requested());
    r.update(|cx| root.update(cx, |d, _| d.open = true));
    frame(&mut r, &mut ui, 1016);
    assert_eq!(width(&ui), 100.);
    assert!(r.animation_frame_requested());
    frame(&mut r, &mut ui, 1066);
    assert_eq!(width(&ui), 150.);
    // Retargeting mid-flight continues from the current width.
    r.update(|cx| root.update(cx, |d, _| d.open = false));
    frame(&mut r, &mut ui, 1091);
    assert_eq!(width(&ui), 175.);
    frame(&mut r, &mut ui, 1141);
    assert_eq!(width(&ui), 137.5);
    frame(&mut r, &mut ui, 1191);
    assert_eq!(width(&ui), 100.);
    assert!(!r.animation_frame_requested());
    let settled = evaluations.get();
    for time in [1207, 1223, 9000] {
        assert!(!r.begin_frame(ms(time)));
        assert!(!ui.needs_prepare(&r).unwrap());
    }
    assert_eq!(evaluations.get(), settled);
    // With reduced motion the value jumps.
    r.set_reduce_motion(true);
    r.update(|cx| root.update(cx, |d, _| d.open = true));
    frame(&mut r, &mut ui, 9016);
    assert_eq!(width(&ui), 200.);
    assert!(!r.animation_frame_requested());
}

#[cfg(feature = "rendering")]
struct Menu {
    anchor: AnchorHandle,
    open: bool,
    modal: bool,
    transition: Option<Transition>,
}
#[cfg(feature = "rendering")]
impl View for Menu {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut page = column().size(400., 300.).child(
            button("Open")
                .key("opener")
                .no_transition()
                .anchor_handle(self.anchor.clone())
                .on_click(cx.listener(|s, _, _| s.open = true)),
        );
        if self.open {
            let content = column().child(
                button("Item")
                    .key("item")
                    .no_transition()
                    .on_click(cx.listener(|s, _, _| s.open = false)),
            );
            let mut overlay = if self.modal {
                modal(content)
            } else {
                popover(self.anchor.clone(), content)
            }
            .key("menu")
            .accessibility_label("Menu")
            .on_dismiss(cx.listener(|s, _: &DismissEvent, _| s.open = false));
            if let Some(transition) = self.transition {
                overlay = overlay.transition(transition);
            }
            page = page.child(overlay);
        }
        page
    }
}
#[cfg(feature = "rendering")]
fn menu(modal: bool) -> (Runtime, Entity<Menu>, Ui<Menu>) {
    let mut r = Runtime::new();
    let root = r.update(|cx| {
        cx.new(|_| Menu {
            anchor: AnchorHandle::new(),
            open: false,
            modal,
            transition: Some(Transition::linear(ms(100))),
        })
    });
    let mut ui = Ui::new(&mut r, root.clone()).unwrap();
    frame(&mut r, &mut ui, 1000);
    (r, root, ui)
}
/// Composition opacity and layer offset of the overlay root containing `key`.
#[cfg(feature = "rendering")]
fn layer<T: View>(ui: &Ui<T>, key: &str) -> Option<(f32, [f32; 2])> {
    let mut id = find(ui, key);
    loop {
        let (parent, opacity) = ui.composition_node(id)?;
        match parent {
            Some(parent) => id = parent,
            None => return Some((opacity, ui.layer_offset(id))),
        }
    }
}
#[cfg(feature = "rendering")]
fn open_menu(r: &mut Runtime, ui: &mut Ui<Menu>, root: &Entity<Menu>, time: u64) {
    r.update(|cx| root.update(cx, |m, _| m.open = true));
    frame(r, ui, time);
}
#[test]
#[cfg(feature = "rendering")]
fn overlays_fade_and_slide_in_while_focus_and_semantics_are_immediate() {
    let (mut r, root, mut ui) = menu(false);
    let opener = find(&ui, "opener");
    assert!(ui.focus(opener));
    open_menu(&mut r, &mut ui, &root, 1016);
    let item = find(&ui, "item");
    // Focus and semantics belong to the open overlay from its first frame.
    assert_eq!(ui.focused_element(), Some(item));
    assert!(ui.semantic_node(item).is_some());
    assert!(ui.needs_composition());
    let offset = Theme::default().sizes().motion.overlay_offset;
    assert_eq!(layer(&ui, "item"), Some((0., [0., -offset])));
    frame(&mut r, &mut ui, 1066);
    assert_eq!(layer(&ui, "item"), Some((0.5, [0., -offset / 2.])));
    // Custom painters see the fade on the overlay root.
    let root = ui.element(find(&ui, "menu")).unwrap();
    assert!(root.viewport_overlay && root.opacity == 0.5);
    frame(&mut r, &mut ui, 1116);
    assert_eq!(layer(&ui, "item"), Some((1., [0., 0.])));
    assert!(!ui.is_animating() && !ui.needs_composition());
}
#[test]
#[cfg(feature = "rendering")]
fn closing_overlays_keep_their_visual_but_release_focus_input_and_semantics() {
    let (mut r, root, mut ui) = menu(false);
    let opener = find(&ui, "opener");
    assert!(ui.focus(opener));
    open_menu(&mut r, &mut ui, &root, 1016);
    frame(&mut r, &mut ui, 1200);
    let item = find(&ui, "item");
    let item_center = center(&ui, "item");
    assert_eq!(ui.focused_element(), Some(item));
    assert!(
        ui.key(
            &mut r,
            KeyEvent {
                physical_key: Default::default(),
                key: KeyboardKey::Escape,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }
        )
        .unwrap()
        .changed
    );
    frame(&mut r, &mut ui, 1216);
    // Focus returns to the opener and the overlay leaves the semantic tree and
    // hit testing at once.
    assert_eq!(ui.focused_element(), Some(opener));
    assert!(ui.semantic_node(item).is_none());
    assert!(ui.semantics().all(|n| n.id != item));
    assert_ne!(ui.hit_test(item_center), Some(item));
    // Only the visual lingers, fading out from where it was.
    let ghost = ui.element(item).unwrap();
    assert!(!ghost.focused && !ghost.hovered && !ghost.focusable);
    assert!(ui.contains_element(item) && ui.needs_composition());
    assert_eq!(layer(&ui, "item").unwrap().0, 1.);
    frame(&mut r, &mut ui, 1266);
    assert_eq!(layer(&ui, "item").unwrap().0, 0.5);
    // Pointer input reaches the content beneath the fading overlay.
    ui.pointer(&mut r, PointerEvent::Moved(item_center))
        .unwrap();
    assert!(!ui.element(item).unwrap().hovered);
    frame(&mut r, &mut ui, 1316);
    assert!(ui.element(item).is_none() && !ui.contains_element(item));
    assert!(!ui.is_animating() && !ui.needs_composition());
    assert!(ui.elements().all(|e| e.key != Some(&Key::from("item"))));
}
#[test]
#[cfg(feature = "rendering")]
fn modal_exit_releases_the_modal_boundary_immediately() {
    let (mut r, root, mut ui) = menu(true);
    open_menu(&mut r, &mut ui, &root, 1016);
    frame(&mut r, &mut ui, 1200);
    let opener = find(&ui, "opener");
    let opener_center = center(&ui, "opener");
    assert_ne!(ui.hit_test(opener_center), Some(opener));
    // A modal only fades; it does not slide.
    r.update(|cx| root.update(cx, |m, _| m.open = false));
    frame(&mut r, &mut ui, 1216);
    assert_eq!(layer(&ui, "item"), Some((1., [0., 0.])));
    assert_eq!(ui.hit_test(opener_center), Some(opener));
    assert!(ui.focus(opener));
    frame(&mut r, &mut ui, 1266);
    assert_eq!(layer(&ui, "item"), Some((0.5, [0., 0.])));
    frame(&mut r, &mut ui, 1316);
    assert!(!ui.is_animating());
}
#[test]
#[cfg(feature = "rendering")]
fn reopening_while_exiting_continues_from_the_fading_visual() {
    let (mut r, root, mut ui) = menu(false);
    open_menu(&mut r, &mut ui, &root, 1016);
    frame(&mut r, &mut ui, 1200);
    r.update(|cx| root.update(cx, |m, _| m.open = false));
    frame(&mut r, &mut ui, 1216);
    frame(&mut r, &mut ui, 1256);
    assert!((layer(&ui, "item").unwrap().0 - 0.6).abs() < 1e-4);
    open_menu(&mut r, &mut ui, &root, 1266);
    // One overlay is painted: the new one, starting at the ghost's opacity.
    assert_eq!(
        ui.elements()
            .filter(|e| e.key == Some(&Key::from("item")))
            .count(),
        1
    );
    assert!((layer(&ui, "item").unwrap().0 - 0.5).abs() < 1e-4);
    frame(&mut r, &mut ui, 1316);
    assert!((layer(&ui, "item").unwrap().0 - 0.75).abs() < 1e-4);
    frame(&mut r, &mut ui, 1366);
    assert!(!ui.is_animating());
}
#[test]
#[cfg(feature = "rendering")]
fn overlays_without_motion_open_and_close_immediately() {
    for (transition, reduce) in [(Some(Transition::None), false), (None, true)] {
        let (mut r, root, mut ui) = menu(false);
        r.update(|cx| root.update(cx, |m, _| m.transition = transition));
        r.set_reduce_motion(reduce);
        open_menu(&mut r, &mut ui, &root, 1016);
        assert_eq!(layer(&ui, "item"), Some((1., [0., 0.])));
        assert!(!ui.is_animating() && !ui.needs_composition());
        let item = find(&ui, "item");
        r.update(|cx| root.update(cx, |m, _| m.open = false));
        frame(&mut r, &mut ui, 1032);
        assert!(!ui.contains_element(item) && !ui.is_animating());
    }
}
#[test]
fn dropping_an_animating_placement_releases_its_frame_request() {
    let (mut r, _, mut ui) = buttons(Buttons {
        transition: linear(),
        ..Default::default()
    });
    let point = center(&ui, "target");
    ui.pointer(&mut r, PointerEvent::Moved(point)).unwrap();
    frame(&mut r, &mut ui, 1016);
    assert!(r.animation_frame_requested());
    drop(ui);
    assert!(!r.animation_frame_requested());
}

#[test]
fn changes_prepared_between_frames_start_at_the_next_frame() {
    let (mut r, root, mut ui) = buttons(Buttons {
        transition: linear(),
        ..Default::default()
    });
    // Input handling prepares long after the last frame, at its stale time.
    r.update(|cx| root.update(cx, |v, _| v.disabled = true));
    ui.prepare(&mut r, [400., 300.], &mut Measure).unwrap();
    assert_eq!(background(&ui, "target"), RED);
    assert!(ui.is_animating() && r.animation_frame_requested());
    frame(&mut r, &mut ui, 5000);
    assert_eq!(background(&ui, "target"), RED);
    frame(&mut r, &mut ui, 5050);
    assert!(close(background(&ui, "target"), [0.5, 0., 0.5, 1.]));
    frame(&mut r, &mut ui, 5100);
    assert_eq!(background(&ui, "target"), BLUE);

    // The same holds for animated values evaluated between frames.
    let evaluations = Rc::new(Cell::new(0));
    let mut r = Runtime::new();
    let drawer = r.update(|cx| {
        cx.new(|_| Drawer {
            open: false,
            evaluations: evaluations.clone(),
        })
    });
    let mut ui = Ui::new(&mut r, drawer.clone()).unwrap();
    frame(&mut r, &mut ui, 1000);
    r.update(|cx| drawer.update(cx, |d, _| d.open = true));
    ui.prepare(&mut r, [400., 300.], &mut Measure).unwrap();
    let width = |ui: &Ui<Drawer>| ui.element(find(ui, "drawer")).unwrap().bounds.width;
    assert_eq!(width(&ui), 100.);
    assert!(r.animation_frame_requested());
    frame(&mut r, &mut ui, 9000);
    assert_eq!(width(&ui), 100.);
    frame(&mut r, &mut ui, 9050);
    assert_eq!(width(&ui), 150.);
    frame(&mut r, &mut ui, 9100);
    assert_eq!(width(&ui), 200.);
    assert!(!r.animation_frame_requested());
}
