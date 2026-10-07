use crate::*;
use taffy::prelude::*;
struct Page(Element);
impl View for Page {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        self.0.clone()
    }
}
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([request.text.len() as f32 * 8., 20.])
    }
}
fn setup(element: Element) -> (Runtime, Entity<Page>, Ui<Page>) {
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| cx.new(|_| Page(element)));
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    (runtime, root, ui)
}
fn keyed<'a, T: View>(ui: &'a Ui<T>, key: &str) -> ElementInfo<'a> {
    ui.elements()
        .find(|e| e.key == Some(&Key::from(key)))
        .unwrap()
}
fn control(key: &str) -> Element {
    button(key)
        .key(key)
        .padding(0.)
        .border(0., [0.; 4])
        .size(80., 30.)
}
#[test]
fn stack_intrinsic_size_is_maximum_in_flow_extent_and_alignment_is_two_dimensional() {
    let (_, _, ui) = setup(
        stack()
            .key("stack")
            .padding(10.)
            .child(column().key("wide").size(120., 20.))
            .child(column().key("tall").size(40., 70.))
            .child(
                column()
                    .key("absolute")
                    .absolute()
                    .size(300., 400.)
                    .left(0.)
                    .top(0.),
            ),
    );
    assert_eq!(keyed(&ui, "stack").bounds.width, 140.);
    assert_eq!(keyed(&ui, "stack").bounds.height, 90.);
    assert_eq!(keyed(&ui, "wide").bounds.x, keyed(&ui, "tall").bounds.x);
    assert_eq!(keyed(&ui, "wide").bounds.y, keyed(&ui, "tall").bounds.y);
    let (_, _, centered) = setup(
        stack()
            .size(200., 100.)
            .align_items(AlignItems::CENTER)
            .justify_items(AlignItems::CENTER)
            .child(column().key("center").size(40., 20.))
            .child(
                column()
                    .key("corner")
                    .size(10., 10.)
                    .align_self(AlignSelf::END)
                    .justify_self(AlignSelf::END),
            ),
    );
    assert_eq!(
        keyed(&centered, "center").bounds,
        Bounds {
            x: 80.,
            y: 40.,
            width: 40.,
            height: 20.
        }
    );
    assert_eq!(keyed(&centered, "corner").bounds.x, 190.);
    assert_eq!(keyed(&centered, "corner").bounds.y, 90.);
}
#[test]
fn stack_places_component_wrappers_in_one_cell_and_live_children_reflow() {
    let mut runtime = Runtime::new();
    let child = runtime.update(|cx| cx.new(|_| Page(column().key("child").size(80., 40.))));
    let root = runtime.update(|cx| {
        cx.new(|_| {
            Page(
                stack()
                    .key("stack")
                    .child(child.clone())
                    .child(column().size(20., 60.)),
            )
        })
    });
    let mut ui = Ui::new(&mut runtime, root).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(keyed(&ui, "stack").bounds.width, 80.);
    assert_eq!(keyed(&ui, "stack").bounds.height, 60.);
    let id = keyed(&ui, "child").id;
    runtime.update(|cx| child.update(cx, |s, _| s.0 = column().key("child").size(100., 100.)));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(keyed(&ui, "stack").bounds.width, 100.);
    assert_eq!(keyed(&ui, "stack").bounds.height, 100.);
    assert_eq!(keyed(&ui, "child").id, id);
}
#[test]
fn flex_percent_constraints_and_axis_spacing_forward_to_taffy() {
    let (_, _, ui) = setup(
        row()
            .size(300., 100.)
            .gap(10.)
            .child(column().key("sidebar").width(80.).fill_height())
            .child(
                column()
                    .key("body")
                    .flex_basis(0.)
                    .flex_grow(1.)
                    .min_width(0.)
                    .fill_height(),
            )
            .child(column().key("right").width_percent(0.2).fill_height()),
    );
    assert_eq!(keyed(&ui, "sidebar").bounds.width, 80.);
    assert_eq!(keyed(&ui, "right").bounds.width, 60.);
    assert_eq!(keyed(&ui, "body").bounds.width, 140.);
    let (_, _, ui) = setup(
        row().size(100., 100.).child(
            column()
                .key("shrink")
                .width(200.)
                .flex_shrink(1.)
                .min_width(0.)
                .max_height(40.)
                .height(100.),
        ),
    );
    assert_eq!(keyed(&ui, "shrink").bounds.width, 100.);
    assert_eq!(keyed(&ui, "shrink").bounds.height, 40.);
    let (_, _, ui) = setup(
        column()
            .key("box")
            .width(200.)
            .min_height(80.)
            .padding_x(12.)
            .padding_y(8.)
            .child(
                column()
                    .key("child")
                    .size(50., 20.)
                    .margin_x(3.)
                    .margin_y(4.),
            ),
    );
    assert_eq!(keyed(&ui, "child").bounds.x, 15.);
    assert_eq!(keyed(&ui, "child").bounds.y, 12.);
    assert_eq!(keyed(&ui, "box").bounds.height, 80.);
}
#[test]
fn positioned_children_anchor_and_stretch_without_claiming_flow_space() {
    let (_, _, ui) = setup(
        column()
            .size(200., 100.)
            .child(column().key("flow").size(30., 20.))
            .child(
                column()
                    .key("corner")
                    .absolute()
                    .size(40., 20.)
                    .top(8.)
                    .right(12.),
            )
            .child(column().key("stretch").absolute().inset(10.))
            .child(
                column()
                    .key("relative")
                    .relative()
                    .size(20., 20.)
                    .left(5.)
                    .top(4.),
            ),
    );
    assert_eq!(
        keyed(&ui, "corner").bounds,
        Bounds {
            x: 148.,
            y: 8.,
            width: 40.,
            height: 20.
        }
    );
    assert_eq!(
        keyed(&ui, "stretch").bounds,
        Bounds {
            x: 10.,
            y: 10.,
            width: 180.,
            height: 80.
        }
    );
    assert_eq!(keyed(&ui, "relative").bounds.y, 24.);
    assert_eq!(keyed(&ui, "relative").bounds.x, 5.);
}
#[test]
fn paint_z_changes_preserve_identity_geometry_tab_and_semantic_order_without_layout() {
    let description = |z| {
        stack()
            .size(100., 100.)
            .child(control("first").z_index(z))
            .child(control("second"))
    };
    let (mut runtime, root, mut ui) = setup(description(0));
    let first = keyed(&ui, "first").id;
    let second = keyed(&ui, "second").id;
    let geometry = keyed(&ui, "first").bounds;
    assert_eq!(ui.hit_test([10., 10.]), Some(second));
    assert!(ui.focus_next(false));
    assert_eq!(ui.focused_element(), Some(first));
    let stats = ui.stats();
    runtime.update(|cx| root.update(cx, |s, _| s.0 = description(5)));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(ui.hit_test([10., 10.]), Some(first));
    assert_eq!(keyed(&ui, "first").bounds, geometry);
    assert_eq!(
        ui.elements()
            .filter(|e| e.key.is_some())
            .map(|e| e.id)
            .collect::<Vec<_>>(),
        [second, first]
    );
    assert_eq!(
        ui.semantics()
            .filter(|e| e.role == SemanticRole::Button)
            .map(|e| e.id)
            .collect::<Vec<_>>(),
        [first, second]
    );
    assert!(ui.focus_next(false));
    assert_eq!(ui.focused_element(), Some(second));
    assert_eq!(ui.stats().measurements, stats.measurements);
    assert_eq!(ui.stats().layout_passes, stats.layout_passes);
    assert_eq!(ui.stats().style_resolutions, stats.style_resolutions);
    runtime.update(|cx| root.update(cx, |s, _| s.0 = description(0)));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(ui.hit_test([10., 10.]), Some(second));
}
#[test]
fn sibling_z_scopes_keep_subtrees_atomic_and_equal_z_uses_description_order() {
    let (_, _, ui) = setup(
        stack()
            .size(100., 100.)
            .child(stack().child(control("deep").z_index(100)).z_index(-1))
            .child(control("top")),
    );
    assert_eq!(ui.hit_test([5., 5.]), Some(keyed(&ui, "top").id));
}
#[test]
fn pointer_block_ignore_disabled_controls_and_clips_follow_paint_order() {
    let (_, _, blocked) = setup(
        stack()
            .size(100., 100.)
            .child(control("under"))
            .child(column().size(40., 30.).pointer_events(PointerEvents::Block)),
    );
    assert_eq!(blocked.hit_test([5., 5.]), None);
    assert_eq!(
        blocked.hit_test([50., 5.]),
        Some(keyed(&blocked, "under").id)
    );
    let (_, _, ignored) = setup(
        stack().size(100., 100.).child(control("under")).child(
            stack()
                .pointer_events(PointerEvents::None)
                .child(control("ignored")),
        ),
    );
    assert_eq!(
        ignored.hit_test([5., 5.]),
        Some(keyed(&ignored, "under").id)
    );
    let (_, _, disabled) = setup(
        stack()
            .size(100., 100.)
            .child(control("under"))
            .child(control("disabled").disabled(true)),
    );
    assert_eq!(disabled.hit_test([5., 5.]), None);
    let (_, _, clipped) = setup(
        stack().size(100., 100.).child(control("under")).child(
            column().size(20., 20.).clip().child(
                column()
                    .size(100., 100.)
                    .pointer_events(PointerEvents::Block),
            ),
        ),
    );
    assert_eq!(clipped.hit_test([10., 10.]), None);
    assert_eq!(
        clipped.hit_test([30., 10.]),
        Some(keyed(&clipped, "under").id)
    );
}
#[test]
fn inert_background_preserves_paint_and_geometry_but_releases_focus_capture_and_semantics() {
    let description = |inert| {
        stack()
            .size(200., 100.)
            .child(column().inert(inert).child(control("background")))
            .child(control("overlay").absolute().left(100.))
    };
    let (mut runtime, root, mut ui) = setup(description(false));
    let background = keyed(&ui, "background").id;
    let bounds = keyed(&ui, "background").bounds;
    ui.pointer(&mut runtime, PointerEvent::Pressed([5., 5.]))
        .unwrap();
    assert_eq!(ui.focused_element(), Some(background));
    runtime.update(|cx| root.update(cx, |s, _| s.0 = description(true)));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(keyed(&ui, "background").id, background);
    assert_eq!(keyed(&ui, "background").bounds, bounds);
    assert!(keyed(&ui, "background").inert);
    assert!(!keyed(&ui, "background").pressed);
    assert_eq!(ui.focused_element(), None);
    assert!(!ui.semantics().any(|n| n.id == background));
    assert_eq!(ui.hit_test([5., 5.]), None);
    assert!(ui.focus_next(false));
    assert_eq!(ui.focused_element(), Some(keyed(&ui, "overlay").id));
    runtime.update(|cx| root.update(cx, |s, _| s.0 = description(false)));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(ui.hit_test([5., 5.]), Some(background));
}
#[test]
fn inert_cancels_preedit_before_layout_and_rejects_later_edits() {
    struct Form {
        value: String,
        inert: bool,
    }
    impl View for Form {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column().inert(self.inert).child(
                text_input(self.value.clone())
                    .key("input")
                    .on_change(cx.listener(|s, e: &TextChangeEvent, _| s.value = e.value.clone())),
            )
        }
    }
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| Form {
            value: "committed".into(),
            inert: false,
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    ui.focus_next(false);
    let id = keyed(&ui, "input").id;
    ui.text_input(
        &mut runtime,
        TextInputEvent::Preedit {
            text: "long preedit".into(),
            cursor: Some((0, 0)),
        },
        &mut Measure,
    )
    .unwrap();
    runtime.update(|cx| root.update(cx, |s, _| s.inert = true));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(keyed(&ui, "input").text, Some("committed"));
    assert_eq!(ui.focused_element(), None);
    assert!(
        !ui.text_input(
            &mut runtime,
            TextInputEvent::Insert("ignored".into()),
            &mut Measure
        )
        .unwrap()
    );
    assert_eq!(keyed(&ui, "input").id, id);
    assert!(ui.is_prepared());
}
#[test]
fn invalid_new_layout_values_fail_and_retry_without_discarding_working_nodes() {
    let (mut runtime, root, mut ui) = setup(column().key("root"));
    for invalid in [
        column().min_width(-1.),
        column().max_height(f32::NAN),
        column().flex_grow(-1.),
        column().left(f32::INFINITY),
        stack().layout(|s| s.grid_auto_rows = vec![taffy::prelude::length(f32::NAN)]),
    ] {
        runtime.update(|cx| root.update(cx, |s, _| s.0 = invalid));
        assert!(matches!(
            ui.prepare(&mut runtime, [800., 600.], &mut Measure),
            Err(UiError::InvalidStyle)
        ));
    }
    runtime.update(|cx| root.update(cx, |s, _| s.0 = column().key("root")));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
}

#[test]
fn inertness_belongs_to_a_placement_even_when_child_state_is_shared() {
    let mut runtime = Runtime::new();
    let child = runtime.update(|cx| cx.new(|_| Page(control("shared"))));
    let blocked_root =
        runtime.update(|cx| cx.new(|_| Page(column().inert(true).child(child.clone()))));
    let live_root = runtime.update(|cx| cx.new(|_| Page(column().child(child.clone()))));
    let mut blocked = Ui::new(&mut runtime, blocked_root).unwrap();
    let mut live = Ui::new(&mut runtime, live_root).unwrap();
    blocked
        .prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    live.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(blocked.hit_test([5., 5.]), None);
    assert_eq!(live.hit_test([5., 5.]), Some(keyed(&live, "shared").id));
    assert!(!blocked.focus_next(false));
    assert!(live.focus_next(false));
    assert!(!blocked.semantics().any(|n| n.role == SemanticRole::Button));
    assert!(live.semantics().any(|n| n.role == SemanticRole::Button));
}

#[test]
fn natural_stack_in_a_column_centers_content_when_cross_axis_stretch_is_disabled() {
    let (_, _, ui) = setup(
        column().width(600.).child(
            stack()
                .key("stack")
                .align_self(AlignSelf::START)
                .padding(12.)
                .border(1., ThemeColor::Border)
                .child(column().key("background").size(280., 80.))
                .child(
                    label("Centered over content")
                        .key("caption")
                        .align_self(AlignSelf::CENTER)
                        .justify_self(AlignSelf::CENTER),
                ),
        ),
    );
    let background = keyed(&ui, "background").bounds;
    let caption = keyed(&ui, "caption").bounds;
    assert_eq!(keyed(&ui, "stack").bounds.width, 306.);
    assert_eq!(
        caption.x + caption.width / 2.,
        background.x + background.width / 2.
    );
    assert_eq!(
        caption.y + caption.height / 2.,
        background.y + background.height / 2.
    );
}

#[test]
fn opacity_is_local_paint_state_and_retains_geometry_measurement_and_interaction() {
    let description = |alpha| {
        column().key("group").opacity(alpha).child(
            button(label("Save").opacity(0.5))
                .key("button")
                .size(80., 30.),
        )
    };
    let (mut runtime, root, mut ui) = setup(description(0.5));
    let group = keyed(&ui, "group").id;
    let control = keyed(&ui, "button").id;
    assert!(ui.needs_composition());
    assert_eq!(keyed(&ui, "button").opacity, 1.);
    assert_eq!(keyed(&ui, "button").parent, Some(group));
    assert!(
        ui.elements()
            .any(|e| e.kind == ElementType::Label && e.opacity == 0.5 && e.parent == Some(control)),
        "button captions with opacity must retain their child group"
    );
    let stats = ui.stats();
    let bounds = keyed(&ui, "button").bounds;
    for alpha in [0., 0.75, 1.] {
        runtime.update(|cx| root.update(cx, |s, _| s.0 = description(alpha)));
        ui.prepare(&mut runtime, [800., 600.], &mut Measure)
            .unwrap();
        assert_eq!(keyed(&ui, "group").id, group);
        assert_eq!(keyed(&ui, "group").opacity, alpha);
        assert_eq!(keyed(&ui, "button").bounds, bounds);
        assert_eq!(ui.stats().layout_passes, stats.layout_passes);
        assert_eq!(ui.stats().measurements, stats.measurements);
        assert_eq!(ui.hit_test([5., 5.]), Some(control));
        assert!(ui.semantics().any(|e| e.id == control));
    }
    runtime.update(|cx| root.update(cx, |s, _| s.0 = column()));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert!(!ui.needs_composition());
}
#[test]
fn invalid_opacity_is_rejected_and_can_be_corrected() {
    let (mut runtime, root, mut ui) = setup(column());
    for opacity in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01, 1.01] {
        runtime.update(|cx| root.update(cx, |s, _| s.0 = column().opacity(opacity)));
        assert!(matches!(
            ui.prepare(&mut runtime, [800., 600.], &mut Measure),
            Err(UiError::InvalidOpacity)
        ));
        runtime.update(|cx| root.update(cx, |s, _| s.0 = column().opacity(0.5)));
        ui.prepare(&mut runtime, [800., 600.], &mut Measure)
            .unwrap();
        assert!(ui.needs_composition());
    }
}
