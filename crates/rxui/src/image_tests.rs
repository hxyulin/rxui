use crate::*;

struct Static(Element);
impl View for Static {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        self.0.clone()
    }
}
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([request.text.chars().count() as f32 * 8., 20.])
    }
}
fn source() -> Image {
    Image::from_rgba8(200, 100, vec![255; 200 * 100 * 4]).unwrap()
}
fn setup(element: Element) -> (Runtime, Entity<Static>, Ui<Static>) {
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
fn image_assets_validate_bytes_and_share_identity_without_dumping_pixels() {
    for (w, h, bytes) in [
        (0, 2, vec![]),
        (2, 0, vec![]),
        (2, 2, vec![0; 15]),
        (u32::MAX, u32::MAX, vec![]),
    ] {
        assert!(matches!(
            Image::from_rgba8(w, h, bytes),
            Err(UiError::InvalidImage)
        ));
    }
    let image = source();
    assert_eq!(image, image.clone());
    assert_ne!(image, source());
    assert_eq!(image.pixel_size(), Some([200, 100]));
    assert_eq!(image.rgba8_pixels().unwrap().len(), 80000);
    assert!(format!("{image:?}").len() < 120);
}
#[test]
fn intrinsic_sizing_crop_fit_alignment_and_semantics_use_the_same_snapshot() {
    let (_, _, ui) = setup(
        column()
            .child(
                image(source())
                    .key("intrinsic")
                    .width(80.)
                    .accessibility_label("Photograph"),
            )
            .child(
                image(source())
                    .key("contain")
                    .width(100.)
                    .height(100.)
                    .image_align(0., 1.),
            )
            .child(
                image(source())
                    .key("cover")
                    .width(100.)
                    .height(100.)
                    .fit(ImageFit::Cover)
                    .image_align(1., 0.),
            )
            .child(
                image(source())
                    .key("crop")
                    .source_region([0., 0., 0.5, 1.])
                    .width(100.)
                    .accessibility_hidden(true),
            ),
    );
    assert_eq!(keyed(&ui, "intrinsic").bounds.height, 40.);
    let contain = keyed(&ui, "contain");
    let draw = contain.image.unwrap();
    assert_eq!(draw.destination.width, 100.);
    assert_eq!(draw.destination.height, 50.);
    assert_eq!(draw.destination.y, contain.bounds.y + 50.);
    assert_eq!(keyed(&ui, "cover").image.unwrap().uv, [0.5, 0., 0.5, 1.]);
    assert_eq!(keyed(&ui, "crop").bounds.height, 100.);
    let id = keyed(&ui, "intrinsic").id;
    let semantic = ui.semantics().find(|n| n.id == id).unwrap();
    assert_eq!(semantic.role, SemanticRole::Image);
    assert_eq!(semantic.label, Some("Photograph"));
    let decorative = keyed(&ui, "crop").id;
    assert!(!ui.semantics().any(|n| n.id == decorative));
    assert_eq!(ui.stats().measurements, 0);
}
#[test]
fn image_paint_changes_and_same_size_replacement_do_not_reflow() {
    let image_source = source();
    let (mut runtime, root, mut ui) = setup(
        image(image_source.clone())
            .key("image")
            .width(100.)
            .height(100.),
    );
    let id = keyed(&ui, "image").id;
    let stats = ui.stats();
    runtime.update(|cx| {
        root.update(cx, |s, _| {
            s.0 = image(image_source.clone())
                .key("image")
                .width(100.)
                .height(100.)
                .fit(ImageFit::Cover)
                .tint(ThemeColor::TextMuted)
                .filter(ImageFilter::Nearest)
        })
    });
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(keyed(&ui, "image").id, id);
    assert_eq!(ui.stats().layout_passes, stats.layout_passes);
    assert_eq!(ui.stats().measurements, 0);
    runtime.update(|cx| {
        root.update(cx, |s, _| {
            s.0 = image(source()).key("image").width(100.).height(100.)
        })
    });
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(ui.stats().layout_passes, stats.layout_passes);
    let before = ui.stats();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(ui.stats(), before);
}
#[test]
fn invalid_crop_and_alignment_fail_before_replacing_working_geometry() {
    let (mut runtime, root, mut ui) = setup(image(source()));
    for bad in [
        image(source()).image_align(f32::NAN, 0.),
        image(source()).source_region([0.8, 0., 0.5, 1.]),
        image(source()).source_region([0., 0., 0., 1.]),
    ] {
        runtime.update(|cx| root.update(cx, |s, _| s.0 = bad));
        assert!(matches!(
            ui.prepare(&mut runtime, [800., 600.], &mut Measure),
            Err(UiError::InvalidImage)
        ));
    }
    runtime.update(|cx| root.update(cx, |s, _| s.0 = image(source())));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
}
#[cfg(feature = "image-decoding")]
#[test]
fn decoding_is_optional_fallible_and_preserves_png_alpha() {
    use image_codec::ImageEncoder;
    let mut bytes = Vec::new();
    image_codec::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &[255, 0, 0, 128],
            1,
            1,
            image_codec::ExtendedColorType::Rgba8,
        )
        .unwrap();
    let image = Image::decode(&bytes).unwrap();
    assert_eq!(image.pixel_size(), Some([1, 1]));
    assert_eq!(image.rgba8_pixels(), Some(&[255, 0, 0, 128][..]));
    assert!(matches!(
        Image::decode(b"invalid"),
        Err(UiError::ImageDecode(_))
    ));
    bytes.clear();
    image_codec::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode(&[255, 0, 0], 1, 1, image_codec::ExtendedColorType::Rgb8)
        .unwrap();
    assert_eq!(Image::decode(&bytes).unwrap().pixel_size(), Some([1, 1]));
}
#[test]
fn composed_buttons_route_child_hits_and_inherit_state_without_layout() {
    struct Page {
        clicks: usize,
        disabled: bool,
    }
    impl View for Page {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            button(
                row()
                    .gap(8.)
                    .child(
                        image(source())
                            .width(20.)
                            .height(20.)
                            .accessibility_hidden(true),
                    )
                    .child(label("Save").key("caption"))
                    .child(label("literal").key("literal").color([1., 0., 0., 1.])),
            )
            .key("button")
            .variant(ButtonVariant::Primary)
            .disabled(self.disabled)
            .on_click(cx.listener(|s, _, _| s.clicks += 1))
        }
    }
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| Page {
            clicks: 0,
            disabled: false,
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    let button = keyed(&ui, "button").id;
    let label = keyed(&ui, "caption").bounds;
    let point = [label.x + 2., label.y + 2.];
    assert_eq!(ui.hit_test(point), Some(button));
    assert_eq!(
        ui.semantics().find(|n| n.id == button).unwrap().label,
        Some("Save literal")
    );
    assert_eq!(
        keyed(&ui, "caption").color,
        Theme::dark().palette().accent_text
    );
    let stats = ui.stats();
    ui.pointer(&mut runtime, PointerEvent::Moved(point))
        .unwrap();
    ui.pointer(&mut runtime, PointerEvent::Pressed(point))
        .unwrap();
    assert_eq!(keyed(&ui, "caption").color, keyed(&ui, "button").color);
    assert_eq!(keyed(&ui, "literal").color, [1., 0., 0., 1.]);
    ui.pointer(&mut runtime, PointerEvent::Released(point))
        .unwrap();
    runtime.update(|cx| assert_eq!(root.read(cx).clicks, 1));
    assert_eq!(ui.stats(), stats);
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    ui.focus_next(false);
    assert_eq!(ui.focused_element(), Some(button));
    runtime.update(|cx| root.update(cx, |s, _| s.disabled = true));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(
        keyed(&ui, "caption").color,
        Theme::dark().palette().text_disabled
    );
    assert_eq!(ui.hit_test(point), None);
    assert_eq!(ui.stats().layout_passes, stats.layout_passes);
}
#[test]
fn composed_names_follow_child_components_and_simple_switches_keep_button_identity() {
    let mut runtime = Runtime::new();
    let child = runtime.update(|cx| cx.new(|_| Static(label("Before"))));
    let root = runtime.update(|cx| cx.new(|_| Static(button(child.clone()).key("action"))));
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    let id = keyed(&ui, "action").id;
    assert_eq!(
        ui.semantics().find(|n| n.id == id).unwrap().label,
        Some("Before")
    );
    runtime.update(|cx| child.update(cx, |s, _| s.0 = label("After")));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
    assert_eq!(
        ui.semantics().find(|n| n.id == id).unwrap().label,
        Some("After")
    );
    assert!(ui.focus_next(false));
    for content in [
        button("Simple").key("action"),
        button(row().child(label("Composed"))).key("action"),
        button("Simple again").key("action"),
    ] {
        runtime.update(|cx| root.update(cx, |s, _| s.0 = content));
        ui.prepare(&mut runtime, [800., 600.], &mut Measure)
            .unwrap();
        assert_eq!(keyed(&ui, "action").id, id);
        assert_eq!(ui.focused_element(), Some(id));
    }
}
#[test]
fn nested_controls_are_rejected_even_when_hidden_in_a_component_and_can_retry() {
    let mut runtime = Runtime::new();
    let child = runtime.update(|cx| cx.new(|_| Static(text_input("Nested"))));
    let root = runtime.update(|cx| cx.new(|_| Static(button(child.clone()))));
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    assert!(matches!(
        ui.prepare(&mut runtime, [800., 600.], &mut Measure),
        Err(UiError::NestedControl)
    ));
    runtime.update(|cx| root.update(cx, |s, _| s.0 = button(row().child(button("Nested")))));
    assert!(matches!(
        ui.prepare(&mut runtime, [800., 600.], &mut Measure),
        Err(UiError::NestedControl)
    ));
    runtime.update(|cx| root.update(cx, |s, _| s.0 = button(row().child(label("Valid")))));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
}
#[test]
fn button_variants_follow_palette_and_allow_explicit_overrides() {
    let (mut runtime, _, mut ui) = setup(
        column()
            .child(
                button("Primary")
                    .key("primary")
                    .variant(ButtonVariant::Primary),
            )
            .child(button("Quiet").key("quiet").variant(ButtonVariant::Quiet))
            .child(
                button("Custom")
                    .key("custom")
                    .variant(ButtonVariant::Primary)
                    .background([1., 0., 0., 1.]),
            ),
    );
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        ui.set_theme(theme.clone()).unwrap();
        ui.prepare(&mut runtime, [800., 600.], &mut Measure)
            .unwrap();
        assert_eq!(
            keyed(&ui, "primary").background,
            Some(theme.palette().accent)
        );
        assert_eq!(keyed(&ui, "primary").color, theme.palette().accent_text);
        assert_eq!(keyed(&ui, "quiet").background, None);
        assert_eq!(keyed(&ui, "custom").background, Some([1., 0., 0., 1.]));
    }
}

#[test]
fn caption_and_invalid_label_children_cannot_bypass_leaf_validation() {
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| cx.new(|_| Static(button("Caption").child(label("Extra")))));
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    assert!(matches!(
        ui.prepare(&mut runtime, [800., 600.], &mut Measure),
        Err(UiError::LeafChildren)
    ));
    runtime.update(|cx| {
        root.update(cx, |s, _| {
            s.0 = button(label("Invalid").child(label("Extra")))
        })
    });
    assert!(matches!(
        ui.prepare(&mut runtime, [800., 600.], &mut Measure),
        Err(UiError::LeafChildren)
    ));
    runtime.update(|cx| root.update(cx, |s, _| s.0 = button(row().child(label("Valid")))));
    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
        .unwrap();
}
