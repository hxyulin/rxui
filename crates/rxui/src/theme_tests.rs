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
        Theme::dark().metrics(|m| m.caption_size = 0.),
        Theme::dark().metrics(|m| m.title_size = f32::INFINITY),
        Theme::dark().metrics(|m| m.heading_weight = FontWeight(0)),
        Theme::dark().metrics(|m| m.radius_large = -1.),
        Theme::dark().metrics(|m| m.radius_small = f32::NAN),
        Theme::dark().metrics(|m| m.row_height = -1.),
        Theme::dark().metrics(|m| m.row_padding_x = f32::NAN),
        Theme::dark().colors(|c| c.row_hover = [0., 0., 0., 1.5]),
        Theme::dark().colors(|c| c.danger_subtle = [f32::NAN; 4]),
        Theme::dark().shadows(|s| s.popover = Some(BoxShadow::new([0.; 4]).blur(-1.))),
        Theme::dark().shadows(|s| s.modal = Some(BoxShadow::new([2.; 4]))),
        Theme::dark().shadows(|s| s.control = Some(BoxShadow::new([0.; 4]).spread(f32::NAN))),
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
fn luminance(c: Color) -> f32 {
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}
fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
/// Composites a possibly translucent color over an opaque one, in linear light.
fn over(top: Color, bottom: Color) -> Color {
    let a = top[3];
    [0, 1, 2, 3].map(|i| {
        if i == 3 {
            1.
        } else {
            top[i] * a + bottom[i] * (1. - a)
        }
    })
}
fn generated() -> Vec<Theme> {
    let mut themes = Vec::new();
    for scheme in [ColorScheme::Dark, ColorScheme::Light] {
        for accent in [
            rgb8(58, 111, 229),  // blue
            rgb8(16, 163, 127),  // teal
            rgb8(255, 199, 0),   // yellow, which takes black accent text
            rgb8(220, 38, 38),   // red
            rgb8(152, 152, 157), // gray
            rgb8(255, 255, 255),
            rgb8(0, 0, 0),
        ] {
            themes.push(Theme::generate(ThemeSeed::new(scheme, accent)));
        }
        themes.push(Theme::generate(
            ThemeSeed::new(scheme, rgb8(147, 51, 234)).neutral(30., 0.03),
        ));
    }
    themes
}
#[test]
fn preset_and_generated_contrast_pairs_and_srgb_conversion_are_verified() {
    assert_eq!(rgb8(0, 255, 0), [0., 1., 0., 1.]);
    let middle = rgb8(128, 128, 128);
    assert!((middle[0] - 0.21586).abs() < 0.0001);
    assert!((rgba8(255, 255, 255, 128)[3] - 128. / 255.).abs() < 0.0001);
    let presets = [Theme::dark(), Theme::light(), Theme::high_contrast()];
    for (i, theme) in presets.iter().chain(&generated()).enumerate() {
        theme.validate().unwrap();
        let c = theme.palette();
        let check = |a: Color, b: Color, min: f32, pair: &str| {
            let ratio = contrast(a, b);
            assert!(ratio >= min, "theme {i}: {pair} is {ratio:.2}:1");
        };
        check(c.text, c.background, 4.5, "text/background");
        check(c.text, c.surface, 4.5, "text/surface");
        check(c.text, c.raised, 4.5, "text/raised");
        check(c.text_muted, c.surface, 4.5, "text_muted/surface");
        check(c.text_muted, c.background, 4.5, "text_muted/background");
        check(c.accent_text, c.accent, 4.5, "accent_text/accent");
        check(
            c.accent_text,
            c.accent_hover,
            4.5,
            "accent_text/accent_hover",
        );
        assert_ne!(c.accent_hover, c.accent, "theme {i}: hover is visible");
        check(
            c.accent_text,
            c.accent_pressed,
            4.5,
            "accent_text/accent_pressed",
        );
        check(
            c.selection_text,
            c.selection,
            4.5,
            "selection_text/selection",
        );
        for fill in [c.background, c.surface, c.control, c.input] {
            check(c.focus, fill, 3., "focus");
        }
        for fill in [c.control, c.control_hover, c.control_pressed] {
            check(c.text, fill, 4.5, "text/control");
        }
        // Rows keep their ordinary text color when hovered or selected.
        for row in [
            over(c.row_hover, c.surface),
            c.row_selected,
            c.row_selected_inactive,
        ] {
            check(c.text, row, 4.5, "text/row");
        }
        for status in [c.danger, c.warning, c.success] {
            check(status, c.surface, 3., "status/surface");
        }
        // Text disabled is intentionally dimmer but still distinct.
        assert!(contrast(c.text_disabled, c.surface) < contrast(c.text_muted, c.surface));
        assert_eq!(c.selection[3], 1.);
    }
    for (scheme, theme) in [
        (
            ColorScheme::Dark,
            Theme::generate(ThemeSeed::new(ColorScheme::Dark, rgb8(0, 122, 255))),
        ),
        (
            ColorScheme::Light,
            Theme::generate(ThemeSeed::new(ColorScheme::Light, rgb8(0, 122, 255))),
        ),
    ] {
        assert_eq!(ColorScheme::of(&theme), scheme);
    }
    // High contrast row selection stands out strongly from its surface.
    let hc = Theme::high_contrast();
    assert!(contrast(hc.palette().row_selected, hc.palette().surface) >= 3.);
}
#[test]
fn generated_themes_derive_states_shadows_and_reject_invalid_seeds() {
    let dark = Theme::generate(ThemeSeed::new(ColorScheme::Dark, rgb8(58, 111, 229)));
    let light = Theme::generate(ThemeSeed::new(ColorScheme::Light, rgb8(58, 111, 229)));
    for theme in [&dark, &light] {
        let c = theme.palette();
        // States step away from the accent text and stay distinct.
        assert_ne!(c.accent, c.accent_hover);
        assert_ne!(c.accent_hover, c.accent_pressed);
        assert_eq!(c.accent_text, [1.; 4]);
        assert!(c.row_hover[3] < 1. && c.danger_subtle[3] < 1.);
        assert_eq!(theme.sizes(), &ThemeMetrics::balanced());
    }
    assert_eq!(dark.elevations(), &ThemeShadows::dark());
    assert_eq!(light.elevations(), &ThemeShadows::light());
    let yellow = Theme::generate(ThemeSeed::new(ColorScheme::Dark, rgb8(255, 199, 0)));
    assert_eq!(yellow.palette().accent_text, [0., 0., 0., 1.]);
    // Pure gray neutrals have no tint; tinted ones differ.
    let gray =
        Theme::generate(ThemeSeed::new(ColorScheme::Dark, rgb8(58, 111, 229)).neutral(0., 0.));
    let s = gray.palette().surface;
    assert!((s[0] - s[1]).abs() < 1e-4 && (s[1] - s[2]).abs() < 1e-4);
    assert_ne!(gray.palette().surface, dark.palette().surface);
    let invalid = Theme::generate(ThemeSeed::new(ColorScheme::Dark, [f32::NAN, 0., 0., 1.]));
    assert!(matches!(invalid.validate(), Err(UiError::InvalidStyle)));
}
#[test]
fn new_semantic_tokens_resolve_in_every_preset() {
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        let c = theme.palette().clone();
        for (token, value) in [
            (ThemeColor::RowHover, c.row_hover),
            (ThemeColor::RowSelected, c.row_selected),
            (ThemeColor::RowSelectedInactive, c.row_selected_inactive),
            (ThemeColor::Danger, c.danger),
            (ThemeColor::Warning, c.warning),
            (ThemeColor::Success, c.success),
            (ThemeColor::DangerSubtle, c.danger_subtle),
            (ThemeColor::WarningSubtle, c.warning_subtle),
            (ThemeColor::SuccessSubtle, c.success_subtle),
        ] {
            assert_eq!(theme.color(token), value);
        }
    }
    assert_eq!(
        Theme::dark().color(ThemeColor::Danger),
        rgb8(0xE5, 0x48, 0x4D)
    );
    assert_eq!(
        Theme::light().color(ThemeColor::RowHover),
        rgba8(0, 0, 0, 10)
    );
    assert_eq!(
        Theme::dark().color(ThemeColor::SuccessSubtle),
        rgba8(0x30, 0xA4, 0x6C, 41)
    );
    let (mut runtime, _, mut ui) = mount(
        column()
            .key("row")
            .background(ThemeColor::RowSelected)
            .border(1., ThemeColor::Danger),
    );
    assert_eq!(
        keyed(&ui, "row").background,
        Some(Theme::dark().palette().row_selected)
    );
    ui.set_theme(Theme::light()).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    let row = keyed(&ui, "row");
    assert_eq!(row.background, Some(Theme::light().palette().row_selected));
    assert_eq!(
        row.paint.border_color,
        Some(Theme::light().palette().danger)
    );
}
#[test]
fn elevations_resolve_per_theme_and_follow_theme_switches() {
    let (mut runtime, _, mut ui) = mount(
        column()
            .child(column().key("menu").elevation(Elevation::Popover))
            .child(column().key("button").shadow(Elevation::Control))
            .child(
                column()
                    .key("patched")
                    .elevation(Elevation::Modal)
                    .hover_style(PaintStyle::new().elevation(Elevation::Control)),
            ),
    );
    let resolved = |theme: &Theme, level| {
        theme.shadow(level).map(|s: BoxShadow| ResolvedShadow {
            offset: s.offset,
            blur: s.blur,
            spread: s.spread,
            color: match s.color {
                StyleColor::Literal(c) => c,
                StyleColor::Theme(t) => theme.color(t),
            },
        })
    };
    let dark = Theme::dark();
    let menu = keyed(&ui, "menu").paint.shadow.unwrap();
    assert_eq!(Some(menu), resolved(&dark, Elevation::Popover));
    assert_eq!((menu.offset, menu.blur), ([0., 8.], 24.));
    assert_eq!(menu.color, rgba8(0, 0, 0, 128));
    // Dark controls are flat.
    assert_eq!(keyed(&ui, "button").paint.shadow, None);
    let light = Theme::light();
    ui.set_theme(light.clone()).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(
        keyed(&ui, "menu").paint.shadow,
        resolved(&light, Elevation::Popover)
    );
    let control = keyed(&ui, "button").paint.shadow.unwrap();
    assert_eq!((control.offset, control.blur), ([0., 1.], 2.));
    assert_eq!(Some(control), resolved(&light, Elevation::Control));
    let modal = keyed(&ui, "patched").paint.shadow.unwrap();
    assert!(modal.blur > menu.blur && modal.offset[1] > menu.offset[1]);
    ui.set_theme(Theme::high_contrast()).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert!(ui.elements().all(|e| e.paint.shadow.is_none()));
    // A customized level re-resolves too.
    let custom = Theme::dark()
        .shadows(|s| s.popover = Some(BoxShadow::new(ThemeColor::Focus).offset(0., 3.).blur(5.)));
    ui.set_theme(custom).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    let menu = keyed(&ui, "menu").paint.shadow.unwrap();
    assert_eq!(
        (menu.offset, menu.blur, menu.color),
        ([0., 3.], 5., Theme::dark().palette().focus)
    );
}
#[test]
fn light_shadows_darken_white_as_much_as_their_css_alphas() {
    // RXUI blends in linear light; browsers blend CSS shadows in sRGB.
    let encode = |v: f32| {
        if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1. / 2.4) - 0.055
        }
    };
    let light = Theme::light();
    let surface = light.palette().surface;
    for (level, css) in [
        (Elevation::Control, 0.06),
        (Elevation::Popover, 0.18),
        (Elevation::Modal, 0.22),
    ] {
        let StyleColor::Literal(color) = light.shadow(level).unwrap().color else {
            panic!("literal shadow color");
        };
        // Full-coverage green channel over white, displayed, against the CSS blend.
        let shown = encode(over(color, surface)[1]);
        let expected = 1. - css * (1. - 24. / 255.);
        assert!(
            (shown - expected).abs() < 0.01,
            "{level:?}: {shown} vs {expected}"
        );
    }
    let alpha = |level| match light.shadow(level).unwrap().color {
        StyleColor::Literal(c) => c[3],
        StyleColor::Theme(_) => unreachable!(),
    };
    assert!(alpha(Elevation::Modal) > alpha(Elevation::Popover));
    // Generated light themes share the preset's shadows.
    let generated = Theme::generate(ThemeSeed::new(ColorScheme::Light, rgb8(0, 122, 255)));
    assert_eq!(generated.elevations(), light.elevations());
}
#[test]
fn text_roles_resolve_from_the_type_scale_and_follow_theme_switches() {
    let (mut runtime, _, mut ui) = mount(
        column()
            .child(label("Title").key("title").text_role(TextRole::Title))
            .child(label("Caption").key("caption").text_role(TextRole::Caption))
            .child(
                column()
                    .text_role(TextRole::Heading)
                    .child(label("Inherited").key("inherited"))
                    .child(label("Own").key("own").font_weight(FontWeight::LIGHT))
                    .child(label("Body").key("body").text_role(TextRole::Body)),
            )
            .child(
                label("Explicit")
                    .key("explicit")
                    .text_role(TextRole::Title)
                    .font_size(30.)
                    .font_weight(FontWeight::BLACK),
            ),
    );
    let m = ThemeMetrics::balanced();
    assert_eq!((m.caption_size, m.font_size), (11., 13.));
    assert_eq!(
        (m.subheading_size, m.heading_size, m.title_size),
        (15., 18., 24.)
    );
    let title = keyed(&ui, "title");
    assert_eq!(title.font_size, 24.);
    assert_eq!(title.text_style.weight, FontWeight::SEMIBOLD);
    let caption = keyed(&ui, "caption");
    assert_eq!(caption.font_size, 11.);
    assert_eq!(caption.text_style.weight, FontWeight::NORMAL);
    let inherited = keyed(&ui, "inherited");
    assert_eq!(inherited.font_size, 18.);
    assert_eq!(inherited.text_style.weight, FontWeight::SEMIBOLD);
    assert_eq!(keyed(&ui, "own").text_style.weight, FontWeight::LIGHT);
    let body = keyed(&ui, "body");
    assert_eq!(
        (body.font_size, body.text_style.weight),
        (13., FontWeight::NORMAL)
    );
    let explicit = keyed(&ui, "explicit");
    assert_eq!(
        (explicit.font_size, explicit.text_style.weight),
        (30., FontWeight::BLACK)
    );
    let revision = keyed(&ui, "title").text_revision;
    ui.set_theme(
        Theme::dark()
            .compact()
            .metrics(|m| m.heading_weight = FontWeight::BOLD),
    )
    .unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    let title = keyed(&ui, "title");
    assert_eq!(title.font_size, ThemeMetrics::compact().title_size);
    assert_eq!(title.text_style.weight, FontWeight::BOLD);
    assert!(title.text_revision > revision);
    assert_eq!(keyed(&ui, "body").font_size, 12.);
    assert_eq!(keyed(&ui, "explicit").font_size, 30.);
}
#[test]
fn preset_buttons_match_input_height_at_every_density() {
    struct Lines;
    impl TextMeasure for Lines {
        fn measure(&mut self, _: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
            Ok([
                request.text.chars().count() as f32 * request.font_size / 2.,
                request.font_size * request.style.line_height,
            ])
        }
    }
    for (theme, height) in [
        (Theme::dark(), 28.),
        (Theme::light().compact(), 24.),
        (Theme::high_contrast(), 28.),
    ] {
        assert_eq!(theme.sizes().input_height, height);
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| {
            cx.new(|_| {
                Static(
                    row()
                        .child(button("Button").key("button"))
                        .child(text_input("Input").key("input")),
                )
            })
        });
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        ui.set_theme(theme).unwrap();
        ui.prepare(&mut runtime, [800., 600.], &mut Lines).unwrap();
        let button = keyed(&ui, "button").bounds.height;
        assert!((button - height).abs() < 1e-3, "{button} != {height}");
        assert_eq!(keyed(&ui, "input").bounds.height, height);
    }
}
#[test]
fn typography_inherits_and_relayouts_when_line_height_changes() {
    struct Lines;
    impl TextMeasure for Lines {
        fn measure(&mut self, _: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
            Ok([
                request.text.chars().count() as f32 * request.font_size / 2.,
                request.font_size * request.style.line_height,
            ])
        }
    }
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| {
            Static(
                column()
                    .font_family("Inter")
                    .font_weight(FontWeight::BOLD)
                    .line_height(2.)
                    .child(label("Inherited").key("inherited"))
                    .child(
                        label("Own")
                            .key("own")
                            .font_style(FontStyle::Italic)
                            .text_align(TextAlign::Center)
                            .line_height(1.),
                    ),
            )
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [400., 300.], &mut Lines).unwrap();
    let inherited = keyed(&ui, "inherited");
    assert_eq!(
        inherited.text_style,
        &TextStyle {
            family: "Inter".into(),
            weight: FontWeight::BOLD,
            style: FontStyle::Normal,
            line_height: 2.,
            align: TextAlign::Start,
            overflow: TextOverflow::Wrap,
        }
    );
    assert_eq!(inherited.bounds.height, inherited.font_size * 2.);
    let own = keyed(&ui, "own");
    assert_eq!(own.text_style.family, FontFamily::from("Inter"));
    assert_eq!(
        (own.text_style.style, own.text_style.align),
        (FontStyle::Italic, TextAlign::Center)
    );
    assert_eq!(own.bounds.height, own.font_size);
    let revision = inherited.text_revision;
    runtime.update(|cx| {
        root.update(cx, |this, _| {
            this.0 = column().child(label("Inherited").key("inherited"));
        })
    });
    ui.prepare(&mut runtime, [400., 300.], &mut Lines).unwrap();
    let inherited = keyed(&ui, "inherited");
    assert_eq!(inherited.text_style, &TextStyle::default());
    assert_eq!(inherited.bounds.height, inherited.font_size * 1.4);
    assert!(inherited.text_revision > revision);
    ui.set_theme(Theme::dark().metrics(|m| m.line_height = 1.))
        .unwrap();
    ui.prepare(&mut runtime, [400., 300.], &mut Lines).unwrap();
    let inherited = keyed(&ui, "inherited");
    assert_eq!(inherited.bounds.height, inherited.font_size);
    for element in [
        label("Bad").font_weight(FontWeight(0)),
        label("Bad").line_height(0.),
        label("Bad").font_family(""),
    ] {
        assert!(matches!(element.validate(), Err(UiError::InvalidStyle)));
    }
    assert!(matches!(
        ui.set_theme(Theme::dark().metrics(|m| m.line_height = f32::NAN)),
        Err(UiError::InvalidStyle)
    ));
}
#[test]
fn custom_theme_tokens_resolve_per_scope_and_validate() {
    let dark = Theme::dark()
        .define_color("sidebar", rgb8(20, 20, 20))
        .define_metric("gutter", 12.);
    let light = Theme::light()
        .define_color("sidebar", rgb8(240, 240, 240))
        .define_color("sidebar", rgb8(230, 230, 230));
    let (mut runtime, _, mut ui) = mount(
        column()
            .child(
                column()
                    .key("dark")
                    .background(ThemeColor::Custom("sidebar")),
            )
            .child(
                column().theme(light.clone()).child(
                    column()
                        .key("light")
                        .background(ThemeColor::Custom("sidebar"))
                        .border(1., ThemeColor::Custom("missing")),
                ),
            ),
    );
    ui.set_theme(dark.clone()).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(keyed(&ui, "dark").background, Some(rgb8(20, 20, 20)));
    let scoped = keyed(&ui, "light");
    assert_eq!(scoped.background, Some(rgb8(230, 230, 230)));
    assert_eq!(scoped.paint.border_color, Some([0.; 4]));
    assert_eq!(dark.custom_metric("gutter"), Some(12.));
    assert_eq!(light.custom_metric("gutter"), None);
    assert_eq!(light.custom_color("missing"), None);
    assert_ne!(dark, Theme::dark());
    for theme in [
        Theme::dark().define_color("bad", [2.; 4]),
        Theme::dark().define_metric("bad", f32::NAN),
    ] {
        assert!(matches!(theme.validate(), Err(UiError::InvalidStyle)));
    }
}
