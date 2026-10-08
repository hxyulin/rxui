use crate::*;

struct Static(Element);
impl View for Static {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        self.0.clone()
    }
}
#[derive(Default)]
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([
            request.text.chars().count() as f32 * request.font_size / 2.,
            request.font_size,
        ])
    }
}
fn mount(element: Element) -> (Runtime, Entity<Static>, Ui<Static>) {
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| cx.new(|_| Static(element)));
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
#[test]
fn inherited_tokens_cross_components_and_resolve_in_local_themes_while_literals_survive() {
    let mut runtime = Runtime::new();
    let child = runtime.update(|cx| {
        cx.new(|_| {
            Static(
                column().child(label("Inherited").key("inherited")).child(
                    label("Explicit")
                        .key("explicit")
                        .color(rgb8(220, 20, 20))
                        .font_size(24.),
                ),
            )
        })
    });
    let scoped = Theme::light().metrics(|m| m.font_size = 20.);
    let root = runtime.update(|cx| {
        cx.new(|_| {
            Static(
                column()
                    .color(ThemeColor::TextMuted)
                    .child(child.clone().into_element().theme(scoped.clone())),
            )
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(keyed(&ui, "inherited").color, scoped.palette().text_muted);
    assert_eq!(keyed(&ui, "inherited").font_size, 20.);
    assert_eq!(keyed(&ui, "explicit").color, rgb8(220, 20, 20));
    assert_eq!(keyed(&ui, "explicit").font_size, 24.);
    runtime.update(|cx| root.update(cx, |this, _| this.0 = this.0.clone().font_size(18.)));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(keyed(&ui, "inherited").font_size, 18.);
    assert_eq!(keyed(&ui, "explicit").font_size, 24.);
    ui.set_theme(Theme::light()).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(keyed(&ui, "inherited").color, scoped.palette().text_muted);
}
#[test]
fn palette_switch_preserves_identity_geometry_text_revision_and_explicit_overrides_without_view_work()
 {
    let (mut runtime, _, mut ui) = mount(
        column()
            .color(ThemeColor::TextMuted)
            .child(label("Token").key("token"))
            .child(
                button("Literal")
                    .key("literal")
                    .background(rgb8(200, 0, 0))
                    .color(rgb8(255, 255, 255)),
            )
            .child(
                text_input("Value")
                    .key("input")
                    .font_size(22.)
                    .padding(0.)
                    .width(300.)
                    .height(60.),
            ),
    );
    let before = ui.stats();
    let identities: Vec<_> = ui
        .elements()
        .map(|e| (e.id, e.bounds, e.text_revision))
        .collect();
    ui.set_theme(Theme::light()).unwrap();
    assert!(!ui.is_prepared());
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(
        ui.stats().component_evaluations,
        before.component_evaluations
    );
    assert_eq!(ui.stats().layout_passes, before.layout_passes);
    assert_eq!(ui.stats().measurements, before.measurements);
    assert_eq!(
        ui.elements()
            .map(|e| (e.id, e.bounds, e.text_revision))
            .collect::<Vec<_>>(),
        identities
    );
    assert_eq!(
        keyed(&ui, "token").color,
        Theme::light().palette().text_muted
    );
    assert_eq!(keyed(&ui, "literal").background, Some(rgb8(200, 0, 0)));
    assert_eq!(keyed(&ui, "literal").color, rgb8(255, 255, 255));
    let input = keyed(&ui, "input");
    assert_eq!(input.font_size, 22.);
    assert_eq!(input.bounds.width, 300.);
    assert_eq!(input.content_bounds.width, 298.); // Explicit zero padding; themed border.
    let stats = ui.stats();
    assert!(!ui.set_theme(Theme::light()).unwrap());
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(ui.stats(), stats);
}
#[test]
fn metric_switch_reflows_only_effective_fonts_and_preserves_explicit_zero_box_metrics() {
    let (mut runtime, _, mut ui) = mount(
        column()
            .child(label("Default").key("default"))
            .child(label("Fixed").key("fixed").font_size(20.))
            .child(button("Default button").key("button"))
            .child(text_input("Default input").key("input"))
            .child(
                button("Fixed box")
                    .key("box")
                    .padding(0.)
                    .border(0., ThemeColor::Border)
                    .width(70.)
                    .height(30.),
            ),
    );
    let default_rev = keyed(&ui, "default").text_revision;
    let fixed_rev = keyed(&ui, "fixed").text_revision;
    let before = ui.stats();
    let theme = Theme::dark().metrics(|m| {
        m.font_size = 18.;
        m.button_padding_x = 20.;
        m.button_padding_y = 20.;
        m.input_padding_x = 14.;
        m.input_padding_y = 14.;
        m.input_width = 310.;
        m.input_height = 64.;
        m.border_width = 3.;
    });
    ui.set_theme(theme).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(
        ui.stats().component_evaluations,
        before.component_evaluations
    );
    assert!(ui.stats().layout_passes > before.layout_passes);
    assert!(keyed(&ui, "default").text_revision > default_rev);
    assert_eq!(keyed(&ui, "fixed").text_revision, fixed_rev);
    assert_eq!(keyed(&ui, "default").font_size, 18.);
    assert_eq!(keyed(&ui, "fixed").font_size, 20.);
    let input = keyed(&ui, "input");
    assert_eq!(input.bounds.width, 310.);
    assert_eq!(input.bounds.height, 64.);
    assert_eq!(input.content_bounds.width, 276.);
    let fixed = keyed(&ui, "box");
    assert_eq!(fixed.bounds.width, 70.);
    assert_eq!(fixed.content_bounds.width, 70.);
}
#[test]
fn state_paints_have_explicit_precedence_and_preserve_focus_without_layout_or_style_work() {
    let (mut runtime, root, mut ui) = mount(
        button("States")
            .key("button")
            .width(120.)
            .height(44.)
            .hover_style(PaintStyle::new().background(rgb8(200, 0, 0)))
            .pressed_style(PaintStyle::new().background(rgb8(0, 200, 0)))
            .disabled_style(PaintStyle::new().background(rgb8(0, 0, 200))),
    );
    let b = keyed(&ui, "button").bounds;
    let point = [b.x + 20., b.y + 20.];
    let stats = ui.stats();
    ui.pointer(&mut runtime, PointerEvent::Moved(point))
        .unwrap();
    assert_eq!(keyed(&ui, "button").background, Some(rgb8(200, 0, 0)));
    ui.pointer(&mut runtime, PointerEvent::Pressed(point))
        .unwrap();
    let button = keyed(&ui, "button");
    assert!(button.focused && button.pressed);
    assert_eq!(button.background, Some(rgb8(0, 200, 0)));
    assert_eq!(button.paint.focus_color, Theme::dark().palette().focus);
    assert_eq!(ui.stats(), stats);
    runtime.update(|cx| root.update(cx, |this, _| this.0 = this.0.clone().disabled(true)));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    let button = keyed(&ui, "button");
    assert!(button.disabled && !button.focused && !button.pressed);
    assert_eq!(button.background, Some(rgb8(0, 0, 200)));
}
#[test]
fn partial_paint_builders_merge_and_explicit_none_removes_default_control_fill() {
    let (mut runtime, _, mut ui) = mount(
        button("No fill")
            .key("button")
            .color(rgb8(255, 0, 0))
            .paint_style(PaintStyle::new().radius(9.).no_background().no_border())
            .hover_style(PaintStyle::new().color(rgb8(0, 255, 0)))
            .hover_style(PaintStyle::new().radius(12.)),
    );
    let button = keyed(&ui, "button");
    assert_eq!(button.background, None);
    assert_eq!(button.paint.border_color, None);
    assert_eq!(button.color, rgb8(255, 0, 0));
    assert_eq!(button.paint.radii, [9.; 4]);
    let point = [button.bounds.x + 5., button.bounds.y + 5.];
    ui.pointer(&mut runtime, PointerEvent::Moved(point))
        .unwrap();
    let button = keyed(&ui, "button");
    assert_eq!(button.color, rgb8(0, 255, 0));
    assert_eq!(button.paint.radii, [12.; 4]);
    assert_eq!(button.background, None);
}
#[test]
fn box_shadows_resolve_tokens_merge_state_patches_and_validate() {
    let (mut runtime, _, mut ui) = mount(
        button("Raised")
            .key("button")
            .shadow(BoxShadow::new(ThemeColor::Focus).offset(0., 2.).blur(6.))
            .hover_style(PaintStyle::new().no_shadow()),
    );
    let button = keyed(&ui, "button");
    let shadow = button.paint.shadow.unwrap();
    assert_eq!(shadow.color, Theme::dark().palette().focus);
    assert_eq!((shadow.offset, shadow.blur), ([0., 2.], 6.));
    assert_eq!(
        shadow.extent(button.bounds).y,
        button.bounds.y + 2. - 9.,
        "extent covers three blur deviations"
    );
    let point = [button.bounds.x + 5., button.bounds.y + 5.];
    ui.pointer(&mut runtime, PointerEvent::Moved(point))
        .unwrap();
    assert_eq!(keyed(&ui, "button").paint.shadow, None);
    for shadow in [
        BoxShadow::new([0.; 4]).blur(-1.),
        BoxShadow::new([0.; 4]).offset(f32::INFINITY, 0.),
        BoxShadow::new([2.; 4]),
    ] {
        assert!(matches!(
            column().shadow(shadow).validate(),
            Err(UiError::InvalidStyle)
        ));
    }
}
#[test]
fn clipping_containers_with_radii_give_descendants_an_inset_rounded_clip() {
    let (_, _, ui) = mount(
        column()
            .key("outer")
            .size(100., 60.)
            .padding(4.)
            .border(2., [1.; 4])
            .corner_radii(10., 4., 0., 30.)
            .clip()
            .child(
                column()
                    .key("inner")
                    .size(50., 50.)
                    .child(label("Deep").key("deep")),
            ),
    );
    let outer = keyed(&ui, "outer");
    assert_eq!(outer.rounded_clip, None);
    let clip = keyed(&ui, "inner").rounded_clip.unwrap();
    assert_eq!(clip.bounds, outer.content_bounds);
    assert_eq!(clip.radii, [4., 0., 0., 24.]);
    assert_eq!(keyed(&ui, "deep").rounded_clip, Some(clip));
    let (_, _, ui) = mount(column().radius(8.).child(label("Unclipped").key("child")));
    assert_eq!(keyed(&ui, "child").rounded_clip, None);
}
#[test]
fn corner_radii_and_single_side_borders_resolve_and_lay_out() {
    let (_, _, ui) = mount(
        column()
            .key("box")
            .corner_radii(1., 2., 3., 4.)
            .border_bottom(3.)
            .border_left(1.)
            .border_color(ThemeColor::Divider)
            .hover_style(PaintStyle::new().radius(5.)),
    );
    let element = keyed(&ui, "box");
    assert_eq!(element.paint.radii, [1., 2., 3., 4.]);
    assert_eq!(element.border, [1., 0., 0., 3.]);
    assert_eq!(
        element.paint.border_color,
        Some(Theme::dark().palette().divider)
    );
    assert!(matches!(
        column().corner_radii(0., f32::NAN, 0., 0.).validate(),
        Err(UiError::InvalidStyle)
    ));
}
#[test]
fn invalid_theme_and_paint_values_are_rejected_before_replacing_a_working_theme() {
    let (_, _, mut ui) = mount(label("Working"));
    for theme in [
        Theme::dark().colors(|c| c.text = [f32::NAN; 4]),
        Theme::dark().metrics(|m| m.font_size = 0.),
        Theme::dark().metrics(|m| m.border_width = -1.),
    ] {
        assert!(matches!(ui.set_theme(theme), Err(UiError::InvalidStyle)));
        assert_eq!(ui.theme(), &Theme::dark());
        assert!(ui.is_prepared());
    }
    for element in [
        button("Bad").radius(-1.),
        label("Bad").color([2.; 4]),
        button("Bad").hover_style(PaintStyle::new().focus_width(f32::NAN)),
        column().theme(Theme::light().metrics(|m| m.input_padding_x = -1.)),
    ] {
        assert!(matches!(element.validate(), Err(UiError::InvalidStyle)));
    }
}
#[test]
fn theme_switches_preserve_placement_scrolling_focus_selection_and_composition() {
    struct Input(String);
    impl View for Input {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column()
                .height(60.)
                .scroll_y()
                .child(label("Top").height(40.))
                .child(text_input(self.0.clone()).key("input").on_change(
                    cx.listener(|this, e: &TextChangeEvent, _| this.0 = e.value.clone()),
                ))
        }
    }
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| cx.new(|_| Input("Shared".into())));
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    let mut other = Ui::new(&mut runtime, root).unwrap();
    for ui in [&mut ui, &mut other] {
        ui.prepare(&mut runtime, [400., 100.], &mut Measure)
            .unwrap();
    }
    ui.focus_next(false);
    ui.text_input(&mut runtime, TextInputEvent::SelectAll, &mut Measure)
        .unwrap();
    ui.text_input(
        &mut runtime,
        TextInputEvent::Preedit {
            text: "你".into(),
            cursor: Some((3, 3)),
        },
        &mut Measure,
    )
    .unwrap();
    ui.prepare(&mut runtime, [400., 100.], &mut Measure)
        .unwrap();
    let focus = ui.focused_element();
    let reset = ui.ime_reset_revision();
    let editing = keyed(&ui, "input").editing.unwrap();
    let scroll = ui.elements().next().unwrap().scroll_offset;
    ui.set_theme(Theme::light()).unwrap();
    ui.prepare(&mut runtime, [400., 100.], &mut Measure)
        .unwrap();
    assert_eq!(ui.focused_element(), focus);
    assert_eq!(ui.ime_reset_revision(), reset);
    let after = keyed(&ui, "input").editing.unwrap();
    assert_eq!(after.selection, editing.selection);
    assert_eq!(after.preedit_range, editing.preedit_range);
    assert_eq!(after.preedit_cursor, editing.preedit_cursor);
    assert_eq!(after.scroll_x, editing.scroll_x);
    assert_eq!(ui.elements().next().unwrap().scroll_offset, scroll);
    assert_eq!(other.theme(), &Theme::dark());
    assert!(other.focused_element().is_none());
}
#[test]
fn preset_contrast_pairs_and_srgb_conversion_are_verified() {
    fn luminance(c: Color) -> f32 {
        c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
    }
    fn contrast(a: Color, b: Color) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    assert_eq!(rgb8(0, 255, 0), [0., 1., 0., 1.]);
    let middle = rgb8(128, 128, 128);
    assert!((middle[0] - 0.21586).abs() < 0.0001);
    assert!((rgba8(255, 255, 255, 128)[3] - 128. / 255.).abs() < 0.0001);
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        theme.validate().unwrap();
        let c = theme.palette();
        for background in [
            c.background,
            c.surface,
            c.control,
            c.control_hover,
            c.control_pressed,
        ] {
            assert!(contrast(c.text, background) >= 7.);
            assert!(contrast(c.text_muted, background) >= 4.5);
            assert!(contrast(c.focus, background) >= 3.);
        }
        assert!(contrast(c.input_border, c.input) >= 3.);
        assert!(contrast(c.selection_text, c.selection) >= 7.);
        for accent in [c.accent, c.accent_hover, c.accent_pressed] {
            assert!(contrast(c.accent_text, accent) >= 4.5);
        }
        assert_eq!(c.selection[3], 1.);
    }
}
