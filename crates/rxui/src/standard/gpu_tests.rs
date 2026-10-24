use crate::*;
use rxui_draw::GraphicsContext;

struct Panel;
impl View for Panel {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .padding(8.)
            .gap(8.)
            .background(ThemeColor::Surface)
            .child(checkbox(true, "").key("on").accessibility_label("On"))
            .child(checkbox(false, "").key("off").accessibility_label("Off"))
            .child(switch(true, "").key("switch").accessibility_label("Switch"))
            .child(slider(0.5).key("slider").width(120.))
            .child(progress_bar(0.5).key("progress").width(120.))
    }
}

/// Linear palette color as written to the linear RGBA8 test target.
fn bytes(c: Color) -> [u8; 4] {
    c.map(|v| (v * 255.).round() as u8)
}
fn close(a: &[u8], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2)
}
/// Whether `a` is nearer `b` than `c`, for antialiased fractional edges.
fn nearer(a: &[u8], b: [u8; 4], c: [u8; 4]) -> bool {
    let d = |x: [u8; 4]| {
        a.iter()
            .zip(x)
            .map(|(a, x)| u32::from(a.abs_diff(x)))
            .sum::<u32>()
    };
    d(b) < d(c)
}

#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn standard_controls_paint_theme_indicator_marks_rails_and_fills_in_each_preset_and_density() {
    let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
    let presets = [Theme::dark(), Theme::light(), Theme::high_contrast()];
    for theme in presets
        .iter()
        .cloned()
        .chain(presets.iter().cloned().map(Theme::compact))
    {
        let mut test = TestUi::new(&graphics, [160, 200], Panel).unwrap();
        test.parts().1.set_theme(theme.clone()).unwrap();
        let pixels = test.render().unwrap();
        let bounds =
            |test: &mut TestUi<Panel>, key: &str| test.element(key).unwrap().unwrap().bounds;
        let at = |x: f32, y: f32| {
            let i = (y as usize * 160 + x as usize) * 4;
            &pixels[i..i + 4]
        };
        let c = theme.palette();
        let m = theme.sizes();
        let accent = bytes(c.accent);
        let (size, mark) = (m.indicator_size, m.indicator_size - 2.);

        // The checked box's content is the tick's frame inside a 1-unit border.
        let on = bounds(&mut test, "on");
        let indicator = [on.x + 4. + 1., on.y + (on.height - size) / 2.];
        let [x, y] = indicator;
        assert!(
            close(at(x + 2., y + 2.), accent),
            "{:?}",
            at(x + 2., y + 2.)
        );
        let tick = [x + 1. + 0.42 * mark, y + 1. + 0.76 * mark];
        assert!(
            nearer(at(tick[0], tick[1]), bytes(c.accent_text), accent),
            "tick {:?}",
            at(tick[0], tick[1])
        );
        // An unchecked box is outlined in TextMuted, which unlike the input
        // border reaches 3:1 against the fill.
        let off = bounds(&mut test, "off");
        let [x, y] = [off.x + 5., off.y + (off.height - size) / 2.];
        assert!(close(at(x + size / 2., y + size / 2.), bytes(c.input)));
        assert!(nearer(
            at(x + size / 2., y + 0.5),
            bytes(c.text_muted),
            bytes(c.input)
        ));

        // A switch that is on keeps its knob at the end of an accent track.
        let switch = bounds(&mut test, "switch");
        let (w, h) = (m.switch_width, m.switch_height);
        let knob = (h * 5. / 8.).round();
        let pad = (h - 2. - knob) / 2.;
        let [x, y] = [switch.x + 5., switch.y + (switch.height - h) / 2.];
        assert!(close(at(x + 6., y + h / 2.), accent));
        let knob_x = x + w - 1. - pad - knob / 2.;
        assert!(close(at(knob_x, y + h / 2.), bytes(c.accent_text)));

        // Half of the slider rail and of the progress track are filled.
        let slider = bounds(&mut test, "slider");
        let rail_y = slider.y + slider.height / 2.;
        assert!(close(at(slider.x + 30., rail_y), accent));
        assert!(close(at(slider.x + 95., rail_y), bytes(c.input)));
        let progress = bounds(&mut test, "progress");
        assert_eq!(progress.height, (h / 2.).floor());
        let mid = progress.y + progress.height / 2.;
        assert!(close(at(progress.x + 20., mid), accent));
        assert!(close(at(progress.x + 100., mid), bytes(c.input)));
    }
}

struct Choices;
impl View for Choices {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .padding(8.)
            .gap(8.)
            .align_items(taffy::AlignItems::START)
            .background(ThemeColor::Surface)
            .child(
                segmented()
                    .key("view")
                    .selected("split")
                    .accessibility_label("View")
                    .option(choice("split", "Split"))
                    .option(choice("raw", "Raw")),
            )
            .child(
                toolbar()
                    .child(
                        toggle_button(true, Icon::TEXT_WRAP)
                            .key("on")
                            .accessibility_label("On"),
                    )
                    .child(
                        toggle_button(false, Icon::TEXT_WRAP)
                            .key("off")
                            .accessibility_label("Off"),
                    ),
            )
    }
}

#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn segmented_tracks_raise_the_selected_segment_and_pressed_toggles_fill_in_each_preset() {
    let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        let mut test = TestUi::new(&graphics, [200, 120], Choices).unwrap();
        let painter = test.painter_mut();
        painter
            .fonts_mut()
            .load_font(include_bytes!("../../tests/fonts/SourceSans3-Regular.otf"))
            .unwrap();
        painter.set_sans_serif_family(Some(std::sync::Arc::from("Source Sans 3")));
        test.parts().1.set_theme(theme.clone()).unwrap();
        let pixels = test.render().unwrap();
        let bounds =
            |test: &mut TestUi<Choices>, key: &str| test.element(key).unwrap().unwrap().bounds;
        let at = |x: f32, y: f32| {
            let i = (y as usize * 200 + x as usize) * 4;
            &pixels[i..i + 4]
        };
        let c = theme.palette();
        // The selected segment's control fill sits inside its border; the
        // unselected one shows the track's input fill.
        let split = bounds(&mut test, "split");
        let mid = split.y + split.height / 2.;
        assert!(
            close(at(split.x + 4., mid), bytes(c.control)),
            "{:?}",
            at(split.x + 4., mid)
        );
        let raw = bounds(&mut test, "raw");
        assert!(
            close(at(raw.x + 4., mid), bytes(c.input)),
            "{:?}",
            at(raw.x + 4., mid)
        );
        // The track's border shows between its edge and the segments.
        let track = bounds(&mut test, "view");
        assert!(close(
            at(track.x + track.width / 2., track.y + 0.5),
            bytes(c.input_border)
        ));
        // A pressed toggle fills with the selected-row color; an unpressed one
        // shows the surface.
        let on = bounds(&mut test, "on");
        assert!(close(
            at(on.x + 3., on.y + on.height / 2.),
            bytes(c.row_selected)
        ));
        let off = bounds(&mut test, "off");
        assert!(close(
            at(off.x + 3., off.y + off.height / 2.),
            bytes(c.surface)
        ));
    }
}
