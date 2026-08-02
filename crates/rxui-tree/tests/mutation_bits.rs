//! Exact invalidation and exact pass cost of every property setter.
//!
//! Two claims are under test for each one. Writing the value an element already
//! holds must dirty nothing, so no pass runs at all; writing a different value
//! must dirty exactly the passes that property can affect, no more. An
//! over-declared bit is measurable waste now that layout, composition, and
//! shaping all prune on these flags, and an under-declared one is a stale pixel.
//!
//! `LAYOUT_ALL` is what a layout-affecting setter is expected to read back.
//! Those setters declare `LAYOUT` alone; `UiTree` expands it to the passes
//! layout feeds.

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize, Physical, Size},
};
use astrelis_paint::{FillRule, Image, ImageSampling, Path, PathBuilder};
use rxui_tree::{
    Align, Alignment, Axis, BoxElement, Button, ButtonIcon, Checkbox, Element, Flex, Frame,
    ImageAlignment, ImageElement, ImageFit, Invalidation, Label, NodeHandle, PassStats, RenderView,
    RenderViewContent, Scroll, ScrollAxis, SemanticData, SemanticRole, Slider, Stack, TextField,
    UiInput, UiTree,
};

fn viewport() -> LogicalSize {
    LogicalSize::new(400.0, 300.0)
}

fn color(value: f32) -> Color {
    Color::new(value, value * 0.5, 1.0 - value, 1.0)
}

fn named(label: &str) -> SemanticData {
    SemanticData {
        role: SemanticRole::Label,
        label: label.into(),
        ..SemanticData::default()
    }
}

/// A box that reports accessible bounds, which is how these tests observe where
/// layout and composition actually put something.
fn marker(label: &str, size: LogicalSize) -> BoxElement {
    BoxElement {
        semantics: Some(named(label)),
        ..BoxElement::new(size, Color::WHITE)
    }
}

fn bounds(ui: &UiTree, label: &str) -> LogicalRect {
    ui.semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == label)
        .unwrap_or_else(|| panic!("missing semantic node for {label}"))
        .bounds
}

/// A settled root holding `element` as the only child of a plain flex.
fn settled<E: Element>(element: E) -> (UiTree, NodeHandle<E>) {
    let mut ui = UiTree::new(Flex::default(), viewport());
    let handle = ui.append(ui.root(), element);
    ui.update_passes();
    (ui, handle)
}

/// Asserts a setter reported a change and dirtied exactly `expected`, then runs
/// the passes so the next assertion starts from a settled tree.
#[track_caller]
fn changed(ui: &mut UiTree, applied: bool, expected: Invalidation) {
    assert!(applied, "the setter reported no change");
    assert_eq!(ui.invalidation(), expected);
    ui.update_passes();
}

/// Asserts a setter found the value already in place, dirtying nothing.
#[track_caller]
fn unchanged(ui: &mut UiTree, applied: bool) {
    assert!(!applied, "the setter reported a change");
    assert_eq!(ui.invalidation(), Invalidation::empty());
}

/// Applies one setter twice: the first call must report a change and dirty
/// exactly `$bits`, the second must report none and dirty nothing.
macro_rules! assert_setter {
    ($ui:ident, $bits:expr, $handle:expr => $method:ident ( $($argument:expr),* $(,)? )) => {{
        let applied = $ui
            .edit($handle)
            .$method($($argument),*);
        changed(&mut $ui, applied, $bits);
        let applied = $ui
            .edit($handle)
            .$method($($argument),*);
        unchanged(&mut $ui, applied);
    }};
}

const CONTROLLED: Invalidation =
    Invalidation::from_bits_retain(Invalidation::PAINT.bits() | Invalidation::ACCESSIBILITY.bits());

const MOVED: Invalidation = Invalidation::from_bits_retain(
    Invalidation::COMPOSE.bits()
        | Invalidation::PAINT.bits()
        | Invalidation::ACCESSIBILITY.bits()
        | Invalidation::HIT_TEST.bits(),
);

fn icon() -> ButtonIcon {
    let mut builder = Path::builder();
    builder
        .move_to(LogicalPoint::new(0.0, 0.0))
        .and_then(|builder| builder.line_to(LogicalPoint::new(16.0, 0.0)))
        .and_then(|builder| builder.line_to(LogicalPoint::new(8.0, 16.0)))
        .and_then(PathBuilder::close)
        .expect("triangle");
    ButtonIcon::new(builder.finish(), LogicalSize::new(16.0, 16.0), 16.0)
        .with_fill_rule(FillRule::NonZero)
}

#[test]
fn label_setters() {
    let (mut ui, label) = settled(Label::new("Ready"));

    assert_setter!(ui, Invalidation::LAYOUT_ALL, label => set_text("Busy"));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, label => set_font_size(18.0));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, label => set_color(Some(color(0.4))));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, label => set_width(Some(120.0)));

    // The reader added for `set_width`, which had none: a consumer cannot skip a
    // redundant write to a property it cannot read back.
    assert_eq!(ui.element(label).width(), Some(120.0));
}

#[test]
fn box_setters() {
    let (mut ui, boxed) = settled(BoxElement::new(LogicalSize::new(40.0, 20.0), Color::WHITE));

    assert_setter!(ui, Invalidation::LAYOUT_ALL, boxed => set_size(LogicalSize::new(60.0, 20.0)));
    assert_setter!(ui, Invalidation::PAINT, boxed => set_color(color(0.2)));
    assert_setter!(ui, Invalidation::ACCESSIBILITY, boxed => set_semantics(Some(named("boxed"))));
    assert_setter!(ui, Invalidation::HIT_TEST, boxed => set_interactive(true));
}

#[test]
fn button_setters() {
    let (mut ui, button) = settled(Button::new(
        "Run",
        LogicalSize::new(80.0, 30.0),
        Color::WHITE,
        Color::BLACK,
        (),
    ));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, button => set_label("Stop"));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, button => set_size(LogicalSize::new(100.0, 30.0)));
    assert_setter!(ui, Invalidation::PAINT, button => set_color(color(0.3)));
    assert_setter!(ui, Invalidation::PAINT, button => set_pressed_color(color(0.6)));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, button => set_text_color(color(0.9)));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, button => set_font_size(16.0));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, button => set_label_visible(false));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, button => set_icon(Some(icon())));
    ui.button_mut(button).set_action(|| Box::new(()));
    assert_eq!(ui.invalidation(), Invalidation::empty());
}

#[test]
fn text_field_setters() {
    let (mut ui, field) = settled(TextField::new("Name", "Astrelis"));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, field => set_label("Project"));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, field => set_text("Astrelis UI"));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, field => set_width(200.0));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, field => set_font_size(16.0));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, field => set_text_color(color(0.7)));
    assert_setter!(ui, Invalidation::PAINT, field => set_background(color(0.1)));
    assert_setter!(ui, Invalidation::PAINT, field => set_selection_color(color(0.5)));
    assert_setter!(ui, Invalidation::PAINT, field => set_caret_color(color(0.8)));
    ui.text_field_mut(field)
        .set_change_action(|text| Box::new(text));
    ui.text_field_mut(field)
        .set_submit_action(|text| Box::new(text));
    assert_eq!(ui.invalidation(), Invalidation::empty());
}

#[test]
fn checkbox_setters() {
    let (mut ui, checkbox) = settled(Checkbox::new("Visible", false, |_| Box::new(())));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, checkbox => set_label("Hidden"));
    assert_setter!(ui, CONTROLLED, checkbox => set_checked(true));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, checkbox => set_size(LogicalSize::new(200.0, 28.0)));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, checkbox => set_text_color(color(0.45)));
    assert_setter!(ui, Invalidation::PAINT, checkbox => set_outline_color(color(0.55)));
    assert_setter!(ui, Invalidation::PAINT, checkbox => set_accent_color(color(0.65)));
    ui.checkbox_mut(checkbox)
        .set_change_action(|checked| Box::new(checked));
    assert_eq!(ui.invalidation(), Invalidation::empty());
}

#[test]
fn slider_setters() {
    let (mut ui, slider) = settled(Slider::new("Volume", 5.0, 0.0..=10.0, |_| Box::new(())));
    assert_setter!(ui, Invalidation::ACCESSIBILITY, slider => set_label("Gain"));
    assert_setter!(ui, CONTROLLED, slider => set_value(7.0));
    assert_setter!(ui, CONTROLLED, slider => set_range(0.0..=20.0));
    assert_setter!(ui, Invalidation::empty(), slider => set_step(2.0));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, slider => set_size(LogicalSize::new(240.0, 28.0)));
    assert_setter!(ui, Invalidation::PAINT, slider => set_track_color(color(0.15)));
    assert_setter!(ui, Invalidation::PAINT, slider => set_accent_color(color(0.85)));
    ui.slider_mut(slider)
        .set_change_action(|value| Box::new(value));
    assert_eq!(ui.invalidation(), Invalidation::empty());
}

#[test]
fn a_guard_compares_the_value_the_element_will_hold() {
    let (mut ui, slider) = settled(Slider::new("Volume", 5.0, 0.0..=10.0, |_| Box::new(())));
    let applied = ui.slider_mut(slider).set_value(100.0);
    changed(&mut ui, applied, CONTROLLED);
    let applied = ui.slider_mut(slider).set_value(50.0);
    unchanged(&mut ui, applied);
    assert_eq!(ui.element(slider).value, 10.0);
    let applied = ui.slider_mut(slider).set_range(10.0..=0.0);
    unchanged(&mut ui, applied);
}

#[test]
fn flex_setters() {
    let (mut ui, flex) = settled(Flex::default());

    assert_setter!(ui, Invalidation::LAYOUT_ALL, flex => set_axis(Axis::Horizontal));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, flex => set_gap(8.0));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, flex => set_padding(4.0));
    assert_setter!(ui, Invalidation::PAINT, flex => set_background(Some(color(0.25))));
}

#[test]
fn stack_setters() {
    let (mut ui, stack) = settled(Stack::default());

    assert_setter!(ui, Invalidation::LAYOUT_ALL, stack => set_padding(6.0));
    assert_setter!(ui, Invalidation::PAINT, stack => set_background(Some(color(0.35))));
}

#[test]
fn frame_setters() {
    let (mut ui, frame) = settled(Frame::default());

    assert_setter!(ui, Invalidation::LAYOUT_ALL, frame => set_width(Some(50.0)));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, frame => set_height(Some(40.0)));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, frame => set_min(LogicalSize::new(10.0, 10.0)));
    assert_setter!(
        ui,
        Invalidation::LAYOUT_ALL,
        frame => set_max(Some(LogicalSize::new(200.0, 200.0)))
    );
    assert_setter!(ui, Invalidation::LAYOUT_ALL, frame => set_grow(1.0));

    // Normalized, so a negative extent is not a second distinct value that
    // re-invalidates on every write.
    let applied = ui.edit(frame).set_min(LogicalSize::new(-4.0, 10.0));
    changed(&mut ui, applied, Invalidation::LAYOUT_ALL);
    let applied = ui.edit(frame).set_min(LogicalSize::new(-9.0, 10.0));
    unchanged(&mut ui, applied);
}

#[test]
fn align_setters() {
    let (mut ui, align) = settled(Align::default());

    assert_setter!(ui, Invalidation::LAYOUT_ALL, align => set_alignment(Alignment::BottomTrailing));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, align => set_padding(8.0));
}

fn image(width: u32, height: u32, seed: u8) -> Image {
    Image::from_rgba8(
        Size::<Physical, u32>::new(width, height),
        vec![seed; (width * height * 4) as usize],
    )
    .expect("valid image")
}

#[test]
fn image_setters() {
    let (mut ui, image_element) = settled(ImageElement::new(image(2, 2, 0), "preview"));
    let replacement = image(3, 4, 255);

    let applied = ui.image_mut(image_element).set_image(replacement.clone());
    changed(
        &mut ui,
        applied,
        Invalidation::PAINT | Invalidation::ACCESSIBILITY,
    );
    let applied = ui.image_mut(image_element).set_image(replacement);
    unchanged(&mut ui, applied);
    assert_setter!(ui, Invalidation::ACCESSIBILITY, image_element => set_label("thumbnail"));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, image_element => set_size(LogicalSize::new(80.0, 60.0)));
    assert_setter!(ui, Invalidation::PAINT, image_element => set_fit(ImageFit::Cover));
    assert_setter!(ui, Invalidation::PAINT, image_element => set_alignment(ImageAlignment::new(0.0, 1.0)));
    assert_setter!(ui, Invalidation::PAINT, image_element => set_sampling(ImageSampling::Nearest));
    assert_setter!(ui, Invalidation::PAINT, image_element => set_opacity(0.5));
}

#[test]
fn render_view_setters() {
    let (mut ui, view) = settled(RenderView::new("scene", LogicalSize::new(80.0, 60.0)));

    assert_setter!(ui, Invalidation::ACCESSIBILITY, view => set_label("viewport"));
    assert_setter!(ui, Invalidation::LAYOUT_ALL, view => set_size(LogicalSize::new(160.0, 90.0)));
    assert_setter!(
        ui,
        Invalidation::PAINT | Invalidation::ACCESSIBILITY,
        view => set_content(RenderViewContent::Error("offline".into()))
    );
    ui.render_view_mut(view)
        .set_input(|input| Box::new(input) as Box<dyn std::any::Any>);
    assert_eq!(
        ui.invalidation(),
        Invalidation::HIT_TEST | Invalidation::ACCESSIBILITY
    );
}

#[test]
fn scroll_setters() {
    let (mut ui, scroll) = settled(Scroll::new(ScrollAxis::Vertical));

    assert_setter!(ui, Invalidation::LAYOUT_ALL, scroll => set_axis(ScrollAxis::Both));
    assert_setter!(
        ui,
        Invalidation::LAYOUT_ALL,
        scroll => set_content_extent(Some(LogicalSize::new(400.0, 1200.0)))
    );
    assert_setter!(ui, MOVED, scroll => set_offset(LogicalPoint::new(0.0, 40.0)));

    ui.edit(scroll)
        .set_scrolled_factory(|offset| Box::new(offset));
    assert_eq!(ui.invalidation(), Invalidation::empty());

    // Readers a consumer needs to drive scrolling at all. Both were private.
    let element = ui.element(scroll);
    assert_eq!(element.viewport(), viewport());
    assert_eq!(element.content(), LogicalSize::new(400.0, 1200.0));
    assert_eq!(element.max_offset(), LogicalPoint::new(0.0, 900.0));
}

#[test]
fn moving_content_within_a_known_extent_costs_no_layout() {
    let mut ui = UiTree::new(Flex::default(), viewport());
    let scroll = ui.append(
        ui.root(),
        Scroll::new(ScrollAxis::Vertical).with_content_extent(LogicalSize::new(400.0, 1200.0)),
    );
    ui.append(
        scroll.id(),
        marker("content", LogicalSize::new(400.0, 1200.0)),
    );
    ui.update_passes();
    assert_eq!(bounds(&ui, "content").origin.y, 0.0);

    let applied = ui.edit(scroll).set_offset(LogicalPoint::new(0.0, 40.0));
    assert!(applied);
    assert_eq!(ui.invalidation(), MOVED);

    let stats = ui.update_passes().stats;
    assert_eq!(stats.layout_elements, 0);
    assert_eq!(
        stats,
        PassStats {
            layout_elements: 0,
            composed_nodes: 3,
            rebuilt_fragments: 1,
            reused_fragments: 2,
            hit_test_nodes: 0,
            accessibility_nodes: 1,
            shaped_text: 0,
            visited_compose_nodes: 3,
            compose_skipped_subtrees: 0,
            visited_accessibility_nodes: 3,
            accessibility_skipped_subtrees: 0,
            invalidate_steps: 1,
        }
    );

    // The content genuinely moved, which is the point of doing it without layout.
    assert_eq!(bounds(&ui, "content").origin.y, -40.0);
}

#[test]
fn a_declared_extent_replaces_the_pseudo_infinite_measuring_pass() {
    let mut ui = UiTree::new(Flex::default(), viewport());
    let scroll = ui.append(ui.root(), Scroll::new(ScrollAxis::Vertical));
    let column = ui.append(scroll.id(), Flex::default());
    let filler = ui.append(
        column.id(),
        Frame {
            grow: 1.0,
            ..Frame::default()
        },
    );
    ui.append(filler.id(), marker("filler", LogicalSize::ZERO));
    ui.update_passes();

    // Undeclared content is measured against a pseudo-infinite box, and anything
    // inside it that grows into what it is offered takes all of it.
    assert_eq!(bounds(&ui, "filler").size.height, 1_000_000.0);

    ui.edit(scroll)
        .set_content_extent(Some(LogicalSize::new(400.0, 900.0)));
    ui.update_passes();
    assert_eq!(bounds(&ui, "filler").size.height, 900.0);
    assert_eq!(
        ui.element(scroll).max_offset(),
        LogicalPoint::new(0.0, 600.0)
    );
}

#[test]
fn a_wheel_tick_reports_the_offset_it_settled_on() {
    let mut ui = UiTree::new(Flex::default(), viewport());
    let scroll = ui.append(
        ui.root(),
        Scroll::new(ScrollAxis::Vertical)
            .with_content_extent(LogicalSize::new(400.0, 1200.0))
            .on_scrolled_factory(|offset| Box::new(offset)),
    );
    ui.append(
        scroll.id(),
        marker("content", LogicalSize::new(400.0, 1200.0)),
    );
    ui.update_passes();

    let action = ui
        .dispatch(UiInput::PointerWheel {
            position: LogicalPoint::new(20.0, 20.0),
            delta: LogicalPoint::new(0.0, 40.0),
        })
        .expect("wheel input is no longer swallowed");
    let offset = *action
        .downcast::<LogicalPoint>()
        .expect("the factory's payload");
    assert_eq!(offset, LogicalPoint::new(0.0, 40.0));
    assert_eq!(ui.element(scroll).offset, offset);

    // The wheel path still asks for layout, unlike `NodeMut::set_offset`: the
    // offset reaches the children as their layout offset, and an element cannot
    // place its children outside its own `layout`.
    assert_eq!(ui.invalidation(), Invalidation::LAYOUT_ALL);
    ui.update_passes();
    assert_eq!(bounds(&ui, "content").origin.y, -40.0);
}

#[test]
fn a_checkbox_toggle_no_longer_relayouts_its_label() {
    let (mut ui, checkbox) = settled(Checkbox::new("Visible", false, |_| Box::new(())));
    let applied = ui.checkbox_mut(checkbox).set_checked(true);
    assert!(applied);
    assert_eq!(ui.invalidation(), CONTROLLED);
    assert_eq!(
        ui.update_passes().stats,
        PassStats {
            layout_elements: 0,
            composed_nodes: 0,
            rebuilt_fragments: 1,
            reused_fragments: 1,
            hit_test_nodes: 0,
            accessibility_nodes: 1,
            shaped_text: 0,
            visited_compose_nodes: 0,
            compose_skipped_subtrees: 0,
            visited_accessibility_nodes: 2,
            accessibility_skipped_subtrees: 0,
            invalidate_steps: 1,
        }
    );
}

fn slider_row() -> (UiTree, NodeHandle<Slider>, NodeHandle<Label>) {
    let mut ui = UiTree::new(Flex::default(), viewport());
    let slider = ui.append(
        ui.root(),
        Slider::new("Volume", 5.0, 0.0..=10.0, |_| Box::new(())),
    );
    let label = ui.append(ui.root(), Label::new("Volume"));
    ui.update_passes();
    (ui, slider, label)
}

#[test]
fn a_slider_drag_no_longer_relayouts_the_row() {
    let (mut ui, slider, label) = slider_row();
    let value = ui.slider_mut(slider).set_value(7.0);
    let text = ui.label_mut(label).set_text("Volume");
    assert!(value);
    assert!(!text);
    assert_eq!(ui.invalidation(), CONTROLLED);
    assert_eq!(
        ui.update_passes().stats,
        PassStats {
            layout_elements: 0,
            composed_nodes: 0,
            rebuilt_fragments: 1,
            reused_fragments: 2,
            hit_test_nodes: 0,
            accessibility_nodes: 1,
            shaped_text: 0,
            visited_compose_nodes: 0,
            compose_skipped_subtrees: 0,
            visited_accessibility_nodes: 2,
            accessibility_skipped_subtrees: 1,
            invalidate_steps: 1,
        }
    );
}

#[test]
fn the_bundles_inherit_the_per_property_guards() {
    let (mut ui, flex) = settled(Flex::default());

    // Only the background differs, so the bundle declares what
    // `set_background` declares rather than layout.
    ui.flex_mut(flex)
        .set_flex(Axis::Vertical, 0.0, 0.0, Some(Color::BLUE));
    assert_eq!(ui.invalidation(), Invalidation::PAINT);
    ui.update_passes();

    // Re-pushing the whole description invalidates nothing.
    ui.flex_mut(flex)
        .set_flex(Axis::Vertical, 0.0, 0.0, Some(Color::BLUE));
    assert_eq!(ui.invalidation(), Invalidation::empty());
}
