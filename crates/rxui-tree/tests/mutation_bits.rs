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
//!
//! Stage 4: dropped button_setters, text_field_setters, checkbox_setters,
//! slider_setters, a_guard_compares_the_value_the_element_will_hold,
//! a_checkbox_toggle_no_longer_relayouts_its_label, and
//! a_slider_drag_no_longer_relayouts_the_row.

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
};
use rxui_tree::{
    Axis, BoxElement, Element, Flex, Frame, Invalidation, Label, NodeHandle, PassStats, Scroll,
    ScrollAxis, SemanticData, SemanticRole, Stack, UiInput, UiTree,
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

const MOVED: Invalidation = Invalidation::from_bits_retain(
    Invalidation::COMPOSE.bits()
        | Invalidation::PAINT.bits()
        | Invalidation::ACCESSIBILITY.bits()
        | Invalidation::HIT_TEST.bits(),
);

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

    // The wheel path still asks for layout, unlike `ElementMut::set_offset`: the
    // offset reaches the children as their layout offset, and an element cannot
    // place its children outside its own `layout`.
    assert_eq!(ui.invalidation(), Invalidation::LAYOUT_ALL);
    ui.update_passes();
    assert_eq!(bounds(&ui, "content").origin.y, -40.0);
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
