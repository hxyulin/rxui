//! Per-element text shaping memoization.
//!
//! `PassStats::shaped_text` counts the calls that reach parley, so it is the
//! direct observable here: a memo hit is a pass that lays a text element out
//! again without moving the counter.
//!
//! Stage 4: dropped typing_one_character_reshapes_the_value_once_and_not_the_placeholder
//! and unwrapped_text_is_not_reshaped_when_its_available_width_changes.

use std::{cell::RefCell, rc::Rc};

use astrelis_core::{
    color::Color,
    geometry::{LogicalRect, LogicalSize},
};
use rxui_tree::{
    Axis, BoxElement, Element, Flex, Frame, KeyedShapingMemo, Label, NodeHandle, UiTree,
};

/// Asks for one element's layout pass without changing any of its properties.
///
/// The property setters are equality-guarded, so re-applying identical content
/// invalidates nothing and no pass runs at all. These tests are about what a
/// layout pass does when it reaches unchanged text, so they request the pass
/// directly rather than provoking it with a redundant write.
fn relayout(ui: &mut UiTree, handle: NodeHandle<Label>) {
    let font_size = ui.element(handle).font_size;
    ui.label_mut(handle).set_font_size(font_size + 1.0);
    ui.label_mut(handle).set_font_size(font_size);
}

fn bounds(ui: &UiTree, label: &str) -> LogicalRect {
    ui.semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == label)
        .unwrap_or_else(|| panic!("missing semantic node for {label}"))
        .bounds
}

/// Every accessible label paired with the size resolved for it. For a text
/// element that size comes from its shaped extent, so comparing two roots this
/// way catches a memo that handed back an entry for the wrong content.
fn measured(ui: &UiTree) -> Vec<(String, LogicalSize)> {
    let mut sizes = ui
        .semantic_snapshot()
        .into_iter()
        .map(|node| (node.data.label, node.bounds.size))
        .collect::<Vec<_>>();
    sizes.sort_by(|left, right| left.0.cmp(&right.0));
    sizes
}

/// Five independently memoized labels, plus a non-text sibling.
struct Tree {
    labels: [NodeHandle<Label>; 5],
}

fn shaping_tree(ui: &mut UiTree) -> Tree {
    let root = ui.root();
    let tree = Tree {
        labels: [
            ui.append(root, Label::new("Ready")),
            ui.append(root, Label::new("Run")),
            ui.append(root, Label::new("Visible")),
            ui.append(root, Label::new("Name")),
            ui.append(root, Label::new("Astrelis")),
        ],
    };
    ui.append(
        root,
        BoxElement::new(LogicalSize::new(20.0, 20.0), Color::WHITE),
    );
    tree
}

/// Re-applies byte-identical content through the property-aware mutation API,
/// which is what framework reconciliation does every frame, then asks for the
/// layout pass those setters no longer request.
fn reconcile_identically(ui: &mut UiTree, tree: &Tree) {
    for (handle, text) in tree
        .labels
        .into_iter()
        .zip(["Ready", "Run", "Visible", "Name", "Astrelis"])
    {
        ui.label_mut(handle)
            .set_content(text.into(), 14.0, Color::WHITE, None);
    }
    assert!(
        !ui.needs_update(),
        "identical content invalidates nothing at all"
    );
    for handle in tree.labels {
        relayout(ui, handle);
    }
}

#[test]
fn a_second_pass_over_unchanged_content_shapes_nothing() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    shaping_tree(&mut ui);

    let first = ui.update_passes().stats;
    // Label, button, checkbox, and both of the field's texts, once each.
    assert_eq!(first.shaped_text, 5);

    // Nothing was invalidated, so layout does not even run.
    let second = ui.update_passes().stats;
    assert_eq!(second.layout_elements, 0);
    assert_eq!(second.shaped_text, 0);
}

#[test]
fn reconciling_identical_content_relayouts_without_reshaping() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    let tree = shaping_tree(&mut ui);
    assert_eq!(ui.update_passes().stats.shaped_text, 5);
    let before = measured(&ui);

    reconcile_identically(&mut ui, &tree);
    let stats = ui.update_passes().stats;
    // The root flex and all five text elements are laid out again - only the
    // untouched box keeps its clean flags and identical constraints - and not one
    // of the four reshapes.
    assert_eq!(stats.layout_elements, 6);
    assert_eq!(stats.shaped_text, 0);

    // The reused layouts measure exactly what they measured when freshly shaped,
    // so a memo that returned the wrong entry would show up as a size change.
    assert_eq!(measured(&ui), before);
}

#[test]
fn a_reused_layout_measures_the_same_as_a_freshly_shaped_one() {
    let mut memoized = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    let tree = shaping_tree(&mut memoized);
    memoized.update_passes();
    reconcile_identically(&mut memoized, &tree);
    assert_eq!(memoized.update_passes().stats.shaped_text, 0);

    // An independent root shapes the same content from a cold memo.
    let mut fresh = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    shaping_tree(&mut fresh);
    assert_eq!(fresh.update_passes().stats.shaped_text, 5);

    assert_eq!(measured(&memoized), measured(&fresh));
}

#[test]
fn a_label_that_only_moves_is_not_reshaped() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Vertical,
            ..Flex::default()
        },
        LogicalSize::new(400.0, 300.0),
    );
    let spacer = ui.append(
        ui.root(),
        BoxElement::new(LogicalSize::new(20.0, 20.0), Color::WHITE),
    );
    let label = ui.append(ui.root(), Label::new("Ready"));
    assert_eq!(ui.update_passes().stats.shaped_text, 1);
    let before = bounds(&ui, "Ready");

    // Grow the sibling, which pushes the label down, and ask for the label's own
    // layout so that it is genuinely laid out again rather than skipped for
    // having clean flags and identical constraints.
    ui.box_mut(spacer).set_size(LogicalSize::new(20.0, 60.0));
    relayout(&mut ui, label);

    let stats = ui.update_passes().stats;
    assert_eq!(stats.shaped_text, 0);
    let after = bounds(&ui, "Ready");
    assert_eq!(after.origin.y, before.origin.y + 40.0);
    assert_eq!(after.size, before.size);
}

#[test]
fn changing_a_labels_text_reshapes_only_that_label() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    let first = ui.append(ui.root(), Label::new("Ready"));
    ui.append(ui.root(), Label::new("Waiting"));
    assert_eq!(ui.update_passes().stats.shaped_text, 2);

    ui.edit(first)
        .set_content("Ready to run".into(), 14.0, Color::WHITE, None);
    assert_eq!(ui.update_passes().stats.shaped_text, 1);

    // The new text is what gets measured, not the memoized old one.
    let mut fresh = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    fresh.append(fresh.root(), Label::new("Ready to run"));
    fresh.update_passes();
    assert_eq!(
        bounds(&ui, "Ready to run").size,
        bounds(&fresh, "Ready to run").size
    );
}

#[test]
fn labels_under_growing_frames_reshape_only_when_their_text_changes() {
    let mut ui = UiTree::new(
        Flex {
            axis: Axis::Horizontal,
            ..Flex::default()
        },
        LogicalSize::new(300.0, 100.0),
    );
    let mut labels = Vec::new();
    for index in 0..3 {
        let frame = ui.append(
            ui.root(),
            Frame {
                grow: 1.0,
                ..Frame::default()
            },
        );
        labels.push(ui.append(frame.id(), Label::new(format!("Row {index}"))));
    }
    // Growth resolution hands each frame a tight constraint, so each label is
    // laid out - and shaped - exactly once.
    assert_eq!(ui.update_passes().stats.shaped_text, 3);

    // Laying the rows out again over identical content shapes nothing.
    for label in &labels {
        relayout(&mut ui, *label);
    }
    let stats = ui.update_passes().stats;
    assert_eq!(stats.layout_elements, 7);
    assert_eq!(stats.shaped_text, 0);

    // Only the row whose text changed reshapes.
    ui.edit(labels[1])
        .set_content("Row one".into(), 14.0, Color::WHITE, None);
    assert_eq!(ui.update_passes().stats.shaped_text, 1);
}

#[test]
fn glyph_color_stays_part_of_the_memo_key() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    let label = ui.append(ui.root(), Label::new("Ready"));
    assert_eq!(ui.update_passes().stats.shaped_text, 1);

    ui.label_mut(label).set_color(Some(Color::BLACK));
    let stats = ui.update_passes().stats;
    assert_eq!(stats.shaped_text, 1);

    // The brush is baked into the shaped glyph runs, so once layout does run a
    // recolored label must reshape. Keying the memo on the whole request is what
    // makes that automatic - the color must not be normalized away.
    ui.edit(label)
        .set_content("Ready".into(), 14.0, Color::BLUE, None);
    assert_eq!(ui.update_passes().stats.shaped_text, 1);
}

#[test]
fn wrapping_text_is_reshaped_when_its_available_width_changes() {
    let text = "a deliberately long sentence that has to wrap more than once";
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(240.0, 300.0));
    ui.append(ui.root(), Label::new(text));
    assert_eq!(ui.update_passes().stats.shaped_text, 1);
    let wide = bounds(&ui, text).size;

    // The guard rail for normalization: a wrapping label's `max_width` decides
    // where its lines break, so a narrower viewport must reshape it.
    ui.set_viewport(LogicalSize::new(90.0, 300.0));
    let stats = ui.update_passes().stats;
    assert_eq!(stats.shaped_text, 1);
    let narrow = bounds(&ui, text).size;
    assert!(
        narrow.height > wide.height,
        "the narrower label wraps onto more lines: {narrow:?} vs {wide:?}"
    );
}

/// Shapes one title per keyed item through [`KeyedShapingMemo`], which is the
/// shape an element whose text count is a collection's length needs.
struct Titles {
    items: Rc<RefCell<Vec<(u32, String)>>>,
    memo: KeyedShapingMemo<u32>,
    shaped: Vec<astrelis_text::TextLayout>,
}

impl Titles {
    fn new(items: &[(u32, &str)]) -> Self {
        Self {
            items: Rc::new(RefCell::new(
                items
                    .iter()
                    .map(|(id, title)| (*id, (*title).to_string()))
                    .collect(),
            )),
            memo: KeyedShapingMemo::default(),
            shaped: Vec::new(),
        }
    }
}

impl Element for Titles {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn layout(
        &mut self,
        context: &mut rxui_tree::LayoutContext<'_>,
        constraints: rxui_tree::Constraints,
    ) -> LogicalSize {
        let items = self.items.borrow();
        let requests = items
            .iter()
            .map(|(id, title)| (*id, astrelis_text::TextLayoutRequest::new(title.clone())));
        self.shaped = self.memo.shape_all(context, requests);
        constraints.max
    }

    fn paint(
        &self,
        _painter: &mut astrelis_paint::Painter,
        _size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        Ok(())
    }
}

#[test]
fn a_keyed_memo_shapes_each_title_once_and_tracks_the_collection() {
    let mut ui = UiTree::new(Flex::default(), LogicalSize::new(400.0, 300.0));
    let element = Titles::new(&[(1, "Source"), (2, "Filter"), (3, "Sink")]);
    let items = Rc::clone(&element.items);
    let titles = ui.append(ui.root(), element);
    ui.update_passes();
    // Three titles, three shapes, and the memo now describes the collection.
    assert_eq!(ui.element(titles).memo.len(), 3);

    let mut width = 400.0;
    let mut relayout_titles = |ui: &mut UiTree| {
        width = if width == 400.0 { 399.0 } else { 400.0 };
        ui.set_viewport(LogicalSize::new(width, 300.0));
    };
    relayout_titles(&mut ui);
    let stats = ui.update_passes().stats;
    assert_eq!(stats.shaped_text, 0, "an unchanged pass shapes nothing");

    // A reorder is free, which is the point of keying by identity rather than by
    // position: an index-keyed memo would miss on every item that moved.
    items.borrow_mut().reverse();
    relayout_titles(&mut ui);
    let stats = ui.update_passes().stats;
    assert_eq!(stats.shaped_text, 0, "a reorder must not reshape");

    // An insertion costs exactly one shape, not one per item after it.
    items.borrow_mut().insert(0, (4, "Probe".into()));
    relayout_titles(&mut ui);
    let stats = ui.update_passes().stats;
    assert_eq!(stats.shaped_text, 1);
    assert_eq!(ui.element(titles).memo.len(), 4);

    // A removal costs none, and drops the entry rather than retaining it: the
    // map tracks the collection, so eviction needs no policy.
    items.borrow_mut().retain(|(id, _)| *id != 4);
    relayout_titles(&mut ui);
    let stats = ui.update_passes().stats;
    assert_eq!(stats.shaped_text, 0);
    assert_eq!(ui.element(titles).memo.len(), 3);

    // Which is observable from the outside: re-adding the same key shapes again,
    // where a retained entry would have hit.
    items.borrow_mut().push((4, "Probe".into()));
    relayout_titles(&mut ui);
    let stats = ui.update_passes().stats;
    assert_eq!(stats.shaped_text, 1);

    // Editing one title reshapes that title alone.
    items.borrow_mut()[0].1 = "Renamed".into();
    relayout_titles(&mut ui);
    let stats = ui.update_passes().stats;
    assert_eq!(stats.shaped_text, 1);
}
