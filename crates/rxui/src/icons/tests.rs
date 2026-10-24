use super::svg_path::{Segment, bounds, parse};
use crate::*;

use Segment::{Close, Cubic, Line, Move, Quad};

fn near(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
}

#[test]
fn path_data_parses_lines_in_absolute_relative_and_implicitly_repeated_forms() {
    assert_eq!(
        parse("M1 2L3 4 5 6h1v-2H0V0z").unwrap(),
        [
            Move([1., 2.]),
            Line([3., 4.]),
            Line([5., 6.]),
            Line([6., 6.]),
            Line([6., 4.]),
            Line([0., 4.]),
            Line([0., 0.]),
            Close,
        ]
    );
    // Pairs after a move are lines, relative after a relative move; a leading
    // relative move is absolute.
    assert_eq!(
        parse("m1 1 2 2M5 5 6 6").unwrap(),
        [
            Move([1., 1.]),
            Line([3., 3.]),
            Move([5., 5.]),
            Line([6., 6.])
        ]
    );
    // Drawing after a close continues from the contour's start.
    assert_eq!(
        parse("M1 1L2 1Z l1 1").unwrap(),
        [
            Move([1., 1.]),
            Line([2., 1.]),
            Close,
            Move([1., 1.]),
            Line([2., 2.]),
        ]
    );
    // Commas and whitespace separate arguments, also between repeated groups.
    assert_eq!(
        parse(" M 1,2 , 3 4\n\tl-1,-1 ").unwrap(),
        [Move([1., 2.]), Line([3., 4.]), Line([2., 3.])]
    );
}

#[test]
fn path_data_parses_compact_numbers_and_exponents() {
    assert_eq!(
        parse("M1.5.5-1-2").unwrap(),
        [Move([1.5, 0.5]), Line([-1., -2.])]
    );
    assert_eq!(
        parse("M1e1-2E-1L+.5-0.25e+1").unwrap(),
        [Move([10., -0.2]), Line([0.5, -2.5])]
    );
}

#[test]
fn smooth_curves_reflect_the_previous_control_point() {
    let path = parse("M0 0C1 2 3 4 5 5S9 8 10 10s1 1 2 2").unwrap();
    assert_eq!(path[2], Cubic([7., 6.], [9., 8.], [10., 10.]));
    // Relative smooth cubic: c2 and end are relative, c1 reflects (9, 8).
    assert_eq!(path[3], Cubic([11., 12.], [11., 11.], [12., 12.]));
    // Without a previous cubic the first control point is the current point.
    assert_eq!(
        parse("M0 0L1 0S2 2 3 3").unwrap()[2],
        Cubic([1., 0.], [2., 2.], [3., 3.])
    );
    let path = parse("M0 0Q1 1 2 0T4 0t2 0").unwrap();
    assert_eq!(path[2], Quad([3., -1.], [4., 0.]));
    assert_eq!(path[3], Quad([5., 1.], [6., 0.]));
    // A cubic does not seed a smooth quadratic.
    assert_eq!(
        parse("M0 0C1 1 2 1 3 0T5 0").unwrap()[2],
        Quad([3., 0.], [5., 0.])
    );
}

/// The point at `t` of a cubic starting at `p0`.
fn cubic_at(p0: [f32; 2], s: Segment, t: f32) -> [f32; 2] {
    let Cubic(c1, c2, p3) = s else {
        panic!("{s:?} is not a cubic")
    };
    let u = 1. - t;
    let f = |i: usize| {
        u * u * u * p0[i] + 3. * u * u * t * c1[i] + 3. * u * t * t * c2[i] + t * t * t * p3[i]
    };
    [f(0), f(1)]
}

#[test]
fn arcs_become_quarter_turn_cubics_on_the_ellipse() {
    // A half circle of radius 1 around (1, 0): sweep 1 turns through -y (up).
    let path = parse("M0 0A1 1 0 0 1 2 0").unwrap();
    assert_eq!(path.len(), 3);
    let Cubic(_, _, mid) = path[1] else { panic!() };
    assert!(near(mid, [1., -1.]), "{mid:?}");
    let Cubic(_, _, end) = path[2] else { panic!() };
    assert_eq!(end, [2., 0.]);
    let mut from = [0., 0.];
    for &s in &path[1..] {
        for t in [0.25, 0.5, 0.75] {
            let [x, y] = cubic_at(from, s, t);
            let r = ((x - 1.).powi(2) + y * y).sqrt();
            assert!((r - 1.).abs() < 1e-3, "radius {r}");
        }
        let Cubic(_, _, end) = s else { panic!() };
        from = end;
    }
    // Sweep 0 turns the other way.
    let Cubic(_, _, mid) = parse("M0 0A1 1 0 0 0 2 0").unwrap()[1] else {
        panic!()
    };
    assert!(near(mid, [1., 1.]), "{mid:?}");

    // The large-arc flag picks three quarters of the circle over one.
    assert_eq!(parse("M0 0A1 1 0 0 1 1 1").unwrap().len(), 2);
    assert_eq!(parse("M0 0A1 1 0 1 1 1 1").unwrap().len(), 4);

    // Radii too small for the endpoints scale up to a half ellipse.
    let path = parse("M0 0a.5 .5 0 0 1 4 0").unwrap();
    let Cubic(_, _, mid) = path[1] else { panic!() };
    assert!(near(mid, [2., -2.]), "{mid:?}");

    // A rotated ellipse: 2 by 1, rotated 90 degrees, so 1 wide and 2 tall.
    let path = parse("M0 0A2 1 90 0 1 0 4").unwrap();
    let Cubic(_, _, mid) = path[1] else { panic!() };
    assert!(near(mid, [1., 2.]), "{mid:?}");

    // Flags need no separators; zero radii draw a line; a zero-length arc nothing.
    let Some(&Cubic(_, _, end)) = parse("M0 0a1 1 0 0012 0").unwrap().last() else {
        panic!()
    };
    assert_eq!(end, [12., 0.]);
    assert_eq!(
        parse("M0 0A0 1 0 0 1 3 4").unwrap(),
        [Move([0., 0.]), Line([3., 4.])]
    );
    assert_eq!(parse("M1 1A1 1 0 0 1 1 1").unwrap(), [Move([1., 1.])]);
}

#[test]
fn malformed_path_data_reports_the_failing_offset() {
    for (d, offset) in [
        ("L1 1", 0),
        ("M1", 2),
        ("M1 2X3", 4),
        ("M1 2L", 5),
        ("M1e 2", 2),
        ("M1 2A1 1 0 2 0 3 3", 11),
        ("M1 2 Z 3", 7),
        ("M--1 2", 1),
        ("M1 2C1 2 3", 10),
        ("M1e99 0", 1),
    ] {
        let error = parse(d).unwrap_err();
        assert_eq!(error.offset(), offset, "{d}: {error}");
        assert!(error.to_string().contains("invalid icon at byte"));
    }
    assert!(parse("").unwrap().is_empty());
    assert!(Icon::from_svg_path([24., 24.], "M1").is_err());
    for view_box in [[0., 24.], [24., -1.], [f32::NAN, 24.], [f32::INFINITY, 1.]] {
        assert!(Icon::from_svg_path(view_box, "M0 0h1").is_err());
    }
}

#[test]
fn every_built_in_icon_parses_inside_its_view_box() {
    let icons: Vec<Icon> = Icon::built_in().collect();
    assert_eq!(icons.len(), 56);
    let mut names = std::collections::HashSet::new();
    for icon in &icons {
        let name = icon.name().unwrap();
        assert!(names.insert(name), "duplicate {name}");
        assert_eq!(icon.view_box(), [24., 24.]);
        assert_eq!(icon.paint(), IconPaint::Stroke);
        let [x0, y0, x1, y1] = icon.bounds().unwrap_or_else(|| panic!("{name} is empty"));
        assert!(
            (x1 - x0).max(y1 - y0) > 1.,
            "{name}: {:?}",
            [x0, y0, x1, y1]
        );
        // Two units of margin hold the stroke's half width.
        assert!(
            x0 >= 1. && y0 >= 1. && x1 <= 23. && y1 <= 23.,
            "{name} leaves its view box: {:?}",
            [x0, y0, x1, y1]
        );
    }
    assert_eq!(Icon::X.name(), Some("x"));
    assert_eq!(Icon::FUNNEL.name(), Some("funnel"));
}

#[test]
fn icons_compare_by_value_and_clone_cheaply() {
    assert_eq!(Icon::SEARCH, Icon::SEARCH);
    assert_ne!(Icon::SEARCH, Icon::X);
    assert_ne!(Icon::SEARCH, Icon::SEARCH.fill());
    assert_eq!(Icon::SEARCH.fill().stroke(), Icon::SEARCH);
    assert!(Icon::SEARCH.shares_geometry(&Icon::SEARCH));

    let plus = Icon::from_svg_path([24., 24.], "M5 12h14M12 5v14").unwrap();
    let copy = plus.clone();
    assert!(copy.shares_geometry(&plus));
    assert_eq!(copy, plus);
    // Separately parsed equal data is equal; other data or view boxes are not.
    assert_eq!(
        Icon::from_svg_path([24., 24.], "M5,12 H19 M12,5 V19").unwrap(),
        plus
    );
    assert_ne!(
        Icon::from_svg_path([16., 16.], "M5 12h14M12 5v14").unwrap(),
        plus
    );
    assert_ne!(Icon::from_svg_path([24., 24.], "M5 12h14").unwrap(), plus);
    assert_eq!(plus.name(), None);
    assert_eq!(plus.clone().fill().paint(), IconPaint::Fill);
    assert!(format!("{:?}", Icon::SEARCH).contains("search"));
    // Unchanged icons keep their measurement, so they never relayout.
    assert!(CustomElement::measure_eq(
        &super::IconMark(Icon::SEARCH),
        &super::IconMark(Icon::SEARCH)
    ));
}

#[cfg(feature = "rendering")]
#[test]
fn prebuilt_paths_equal_only_their_clones() {
    let path = || {
        let mut b = rxui_draw::Path::builder();
        b.move_to([0., 0.]).line_to([1., 1.]);
        b.build().unwrap()
    };
    let a = Icon::from_path([2., 2.], path(), IconPaint::Fill).unwrap();
    assert_eq!(a.clone(), a);
    assert_ne!(
        Icon::from_path([2., 2.], path(), IconPaint::Fill).unwrap(),
        a
    );
    assert_eq!(a.paint(), IconPaint::Fill);
    assert!(Icon::from_path([0., 2.], path(), IconPaint::Fill).is_err());
}

#[test]
fn segment_bounds_cover_curves() {
    let b = bounds(&parse("M0 0Q5 10 10 0").unwrap()).unwrap();
    assert!((b[3] - 5.).abs() < 0.01, "{b:?}");
    assert_eq!(bounds(&parse("M3 3").unwrap()), None);
}

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.chars().count() as f32 * 8., 20.])
    }
}

struct Icons {
    name_button: bool,
}
impl View for Icons {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let search = icon_button(Icon::SEARCH).key("search");
        column()
            .padding(10.)
            .gap(10.)
            .child(icon(Icon::SEARCH).key("plain"))
            .child(icon(Icon::X).key("named").accessibility_label("Close"))
            .child(icon(Icon::PLUS).key("sized").size(24., 24.))
            .child(
                button("Run")
                    .key("run")
                    .leading_icon(Icon::PLAY)
                    .width(200.)
                    .height(40.),
            )
            .child(button("More").key("more").trailing_icon(Icon::CHEVRON_DOWN))
            .child(label("Saved").key("saved").leading_icon(Icon::CHECK))
            .child(if self.name_button {
                search.accessibility_label("Search")
            } else {
                search
            })
    }
}

struct Harness {
    r: Runtime,
    ui: Ui<Icons>,
}
impl Harness {
    fn new() -> Self {
        let mut r = Runtime::new();
        let e = r.update(|cx| cx.new(|_| Icons { name_button: true }));
        let mut ui = Ui::new(&mut r, e).unwrap();
        ui.prepare(&mut r, [400., 600.], &mut Measure).unwrap();
        Self { r, ui }
    }
    fn prepare(&mut self) {
        self.ui
            .prepare(&mut self.r, [400., 600.], &mut Measure)
            .unwrap();
    }
    fn id(&self, key: &str) -> ElementId {
        let key = Key::from(key);
        self.ui.elements().find(|e| e.key == Some(&key)).unwrap().id
    }
    fn bounds(&self, key: &str) -> Bounds {
        self.ui.element(self.id(key)).unwrap().bounds
    }
    /// Children of the element with `key`, in order.
    fn children(&self, key: &str) -> Vec<ElementInfo<'_>> {
        let id = self.id(key);
        self.ui
            .elements()
            .filter(|e| e.parent == Some(id))
            .collect()
    }
}

#[test]
fn icon_size_follows_theme_density_live_and_explicit_sizes_win() {
    let mut h = Harness::new();
    let side = |h: &Harness, key| {
        let b = h.bounds(key);
        [b.width, b.height]
    };
    assert_eq!(side(&h, "plain"), [16., 16.]);
    assert_eq!(side(&h, "sized"), [24., 24.]);
    assert_eq!(side(&h, "search"), [28., 28.]);
    h.ui.set_theme(Theme::dark().compact()).unwrap();
    h.prepare();
    assert_eq!(side(&h, "plain"), [14., 14.]);
    assert_eq!(side(&h, "sized"), [24., 24.]);
    assert_eq!(side(&h, "search"), [24., 24.]);
    // The icon button's icon follows the theme too, centered in the square.
    let inner = h.children("search")[0].bounds;
    let outer = h.bounds("search");
    assert_eq!([inner.width, inner.height], [14., 14.]);
    assert_eq!(inner.x - outer.x, (24. - 14.) / 2.);
    assert_eq!(inner.y - outer.y, (24. - 14.) / 2.);
    h.ui.set_theme(Theme::light().metrics(|m| m.icon_size = 20.))
        .unwrap();
    h.prepare();
    assert_eq!(side(&h, "plain"), [20., 20.]);
    assert!(
        Theme::dark()
            .metrics(|m| m.icon_size = -1.)
            .validate()
            .is_err()
    );
}

#[test]
fn icons_are_hidden_from_assistive_technology_until_named() {
    let h = Harness::new();
    assert!(h.ui.semantic_node(h.id("plain")).is_none());
    let named = h.ui.semantic_node(h.id("named")).unwrap();
    assert_eq!(named.role, SemanticRole::Image);
    assert_eq!(named.label, Some("Close"));
    // Named after explicitly hiding stays visible; hiding after naming hides.
    let element = icon(Icon::X).accessibility_label("Close");
    assert!(!element.semantics.as_ref().unwrap().hidden);
    let element = icon(Icon::X)
        .accessibility_label("Close")
        .accessibility_hidden(true);
    assert!(element.semantics.as_ref().unwrap().hidden);
    // Naming other elements does not change their visibility.
    let element = label("x")
        .accessibility_hidden(true)
        .accessibility_label("y");
    assert!(element.semantics.as_ref().unwrap().hidden);

    // Captions keep naming buttons with icons; icon buttons use their label.
    let run = h.ui.semantic_node(h.id("run")).unwrap();
    assert_eq!((run.role, run.label), (SemanticRole::Button, Some("Run")));
    let search = h.ui.semantic_node(h.id("search")).unwrap();
    assert_eq!(
        (search.role, search.label),
        (SemanticRole::Button, Some("Search"))
    );
    // The icon of a labelled row stays hidden beside its caption.
    let saved = h.children("saved");
    assert!(h.ui.semantic_node(saved[0].id).is_none());
    assert!(h.ui.semantic_node(saved[1].id).is_some());
}

#[test]
fn icon_buttons_without_an_accessible_name_are_rejected() {
    let mut r = Runtime::new();
    let e = r.update(|cx| cx.new(|_| Icons { name_button: false }));
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    let error = ui.prepare(&mut r, [400., 600.], &mut Measure).unwrap_err();
    assert!(matches!(error, UiError::MissingAccessibleName), "{error}");
    r.update(|cx| e.update(cx, |s, _| s.name_button = true));
    ui.prepare(&mut r, [400., 600.], &mut Measure).unwrap();
    // A labelled-by relationship or a hidden button also passes.
    let handle = AccessibilityHandle::new();
    assert!(
        icon_button(Icon::X)
            .accessibility_labelled_by(handle)
            .validate()
            .is_ok()
    );
    assert!(
        icon_button(Icon::X)
            .accessibility_hidden(true)
            .validate()
            .is_ok()
    );
}

#[test]
fn leading_and_trailing_icons_center_a_row_of_icon_and_caption() {
    let h = Harness::new();
    let run = h.bounds("run");
    let parts = h.children("run");
    assert_eq!(parts.len(), 2);
    let (mark, caption) = (parts[0].bounds, parts[1].bounds);
    assert_eq!(parts[0].kind, ElementType::Custom);
    assert_eq!(parts[1].text, Some("Run"));
    // 16 + 6 + 24 units of content centered in 200.
    assert_eq!(caption.x - (mark.x + mark.width), 6.);
    let left = mark.x - run.x;
    let right = run.x + run.width - (caption.x + caption.width);
    assert!((left - right).abs() < 0.01, "{left} {right}");
    let center = |b: Bounds| b.y + b.height / 2.;
    assert!((center(mark) - center(run)).abs() < 0.01);
    assert!((center(caption) - center(run)).abs() < 0.01);

    let more = h.children("more");
    assert_eq!(more[0].text, Some("More"));
    assert_eq!(more[1].kind, ElementType::Custom);
    // A label becomes an aligned row keeping its key.
    let saved = h.children("saved");
    assert_eq!(saved[0].kind, ElementType::Custom);
    assert_eq!(saved[1].text, Some("Saved"));
    assert!((center(saved[0].bounds) - center(saved[1].bounds)).abs() < 0.01);
}

#[cfg(feature = "rendering")]
mod gpu {
    use crate::*;
    use rxui_draw::GraphicsContext;

    struct Panel {
        disabled: bool,
    }
    impl View for Panel {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column().padding(8.).background(ThemeColor::Surface).child(
                icon_button(Icon::SQUARE)
                    .key("button")
                    .accessibility_label("Stop")
                    .disabled(self.disabled)
                    .no_transition(),
            )
        }
    }

    fn bytes(c: Color) -> [u8; 4] {
        c.map(|v| (v * 255.).round() as u8)
    }
    fn close(a: &[u8], b: [u8; 4]) -> bool {
        a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2)
    }

    #[test]
    #[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
    fn icons_paint_in_the_resolved_text_color_and_follow_hover_and_disabled_states() {
        let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
        // A 48-unit icon makes the stroke 4 units wide; the button is 56 square.
        let theme = Theme::dark().metrics(|m| {
            m.icon_size = 48.;
            m.input_height = 56.;
        });
        let c = theme.palette().clone();
        let mut test = TestUi::new(&graphics, [80, 80], Panel { disabled: false }).unwrap();
        test.parts().1.set_theme(theme).unwrap();
        let sample = |test: &mut TestUi<Panel>| {
            let b = test.element("button").unwrap().unwrap().bounds;
            let mark = [b.x + 4., b.y + 4.];
            let pixels = test.render().unwrap();
            let at = |x: f32, y: f32| {
                let i = (y as usize * 80 + x as usize) * 4;
                [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
            };
            // The square's left edge runs at x = 3 of 24, two units wide there:
            // pixels 4..8 of the 48-unit icon. Its middle is empty.
            let edge = at(mark[0] + 6., mark[1] + 24.);
            let inside = at(mark[0] + 24., mark[1] + 24.);
            let outside = at(mark[0] + 1., mark[1] + 24.);
            (
                edge,
                inside,
                outside,
                [b.x + b.width / 2., b.y + b.height / 2.],
            )
        };
        let (edge, inside, outside, center) = sample(&mut test);
        // Quiet buttons rest in TextMuted with no fill.
        assert!(close(&edge, bytes(c.text_muted)), "{edge:?}");
        assert!(close(&inside, bytes(c.surface)), "{inside:?}");
        assert!(close(&outside, bytes(c.surface)), "{outside:?}");

        test.pointer(PointerEvent::Moved(center)).unwrap();
        let (edge, ..) = sample(&mut test);
        assert!(close(&edge, bytes(c.text)), "hovered {edge:?}");

        let entity = test.entity().clone();
        test.runtime()
            .update(|cx| entity.update(cx, |s, _| s.disabled = true));
        let (edge, ..) = sample(&mut test);
        assert!(close(&edge, bytes(c.text_disabled)), "disabled {edge:?}");
    }
}
