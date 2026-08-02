//! Differential equivalence between incremental and full composition.
//!
//! Composition is incremental: a subtree is skipped whenever its parent
//! produced the same world transform and clip as last update and the subtree
//! carries no compose work. A *missed descent* - a subtree that should have
//! been recomposed but was skipped - leaves stale `world_transform`,
//! `world_clip`, `world_inverse`, and `subtree_bounds` behind, which surfaces
//! as fragments drawn in the wrong place and pointers landing on the wrong
//! element. Neither is visible to a test that only inspects the incremental
//! tree, because the stale values are self-consistent.
//!
//! So these tests compare the incrementally updated tree against a *freshly
//! built* tree in the same logical state. A fresh tree has every node dirty,
//! so its composition is necessarily a full correct walk, and any divergence
//! is a missed descent.
//!
//! The whole comparison runs off one description type, [`Spec`]: it holds each
//! element's complete logical state, a mutation edits the description and the
//! live tree from the same values, and [`instantiate`] turns the description
//! into a tree. Nothing is compared by [`NodeId`], since each tree allocates
//! its own; everything is keyed on pre-order position, which [`instantiate`]
//! makes mean the same logical node in both trees.
//!
//! Every tree here uses [`FontDatabase::empty`]: shaping stays deterministic
//! and a fresh tree costs microseconds instead of the ~48ms that discovering
//! system fonts would add to each of the ~5000 trees built below.

use std::{
    any::Any,
    cell::Cell,
    collections::{HashMap, HashSet},
};

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
    math::{Affine2, Vec2},
};
use astrelis_text::FontDatabase;
use rxui_tree::{
    Align, Alignment, Axis, BoxElement, Constraints, Element, Flex, Frame, Label, LayoutContext,
    NodeHandle, NodeId, PassStats, Scroll, ScrollAxis, SemanticActionKind, SemanticData,
    SemanticRole, Stack, UiInput, UiTree,
};

// ---------------------------------------------------------------------------
// transforming element
// ---------------------------------------------------------------------------

/// Container with a mutable non-translation transform and a mutable clip.
///
/// No element in this crate overrides [`Element::transform`], so without one
/// here every generated tree would compose pure translations and would never
/// exercise `world_inverse` against a rotation. It also supplies the mutation
/// the skip rule cares about most: a change that alters only what children
/// inherit, declaring [`Invalidation::COMPOSE`] without `LAYOUT`, so descent
/// can only happen through `inherited_changed`.
struct Spin {
    angle: Cell<f32>,
    scale: Cell<f32>,
    clip: Cell<bool>,
}

impl Element for Spin {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let mut content = LogicalSize::ZERO;
        for child in context.children() {
            let size = context.layout_child(child, constraints.loosen());
            context.place_child(child, LogicalPoint::ZERO);
            content.width = content.width.max(size.width);
            content.height = content.height.max(size.height);
        }
        constraints.constrain(content)
    }

    fn transform(&self) -> Affine2 {
        Affine2::from_angle(self.angle.get()) * Affine2::from_scale(Vec2::splat(self.scale.get()))
    }

    fn clips_children(&self) -> bool {
        self.clip.get()
    }
}

// ---------------------------------------------------------------------------
// deterministic generator
// ---------------------------------------------------------------------------

/// xorshift64\* with a multiplicative output stage.
///
/// Written inline so the test needs no dependency and so a failure is
/// reproducible from the printed seed alone.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Mixing keeps small seeds out of the near-zero state that makes plain
        // xorshift correlate across its first outputs.
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        self.0 = state;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            ((self.next_u64() >> 32) as u32) % bound
        }
    }

    fn flag(&mut self) -> bool {
        self.below(2) == 1
    }

    /// Whole number in `low..=high`, so failing geometry reads cleanly.
    fn whole(&mut self, low: i32, high: i32) -> f32 {
        let span = (high - low + 1).max(1) as u32;
        (low + self.below(span) as i32) as f32
    }

    fn eighth(&mut self) -> f32 {
        self.below(9) as f32 / 8.0
    }

    fn color(&mut self) -> Color {
        Color::new(self.eighth(), self.eighth(), self.eighth(), 1.0)
    }

    fn size(&mut self) -> LogicalSize {
        LogicalSize::new(self.whole(2, 120), self.whole(2, 90))
    }

    fn word(&mut self) -> String {
        const WORDS: [&str; 8] = [
            "alpha",
            "beta",
            "gamma",
            "delta",
            "epsilon zeta",
            "eta",
            "theta iota",
            "",
        ];
        WORDS[self.below(WORDS.len() as u32) as usize].to_owned()
    }

    fn axis(&mut self) -> Axis {
        if self.flag() {
            Axis::Horizontal
        } else {
            Axis::Vertical
        }
    }

    fn scroll_axis(&mut self) -> ScrollAxis {
        match self.below(3) {
            0 => ScrollAxis::Vertical,
            1 => ScrollAxis::Horizontal,
            _ => ScrollAxis::Both,
        }
    }

    fn alignment(&mut self) -> Alignment {
        match self.below(9) {
            0 => Alignment::TopLeading,
            1 => Alignment::Top,
            2 => Alignment::TopTrailing,
            3 => Alignment::Leading,
            4 => Alignment::Center,
            5 => Alignment::Trailing,
            6 => Alignment::BottomLeading,
            7 => Alignment::Bottom,
            _ => Alignment::BottomTrailing,
        }
    }

    fn optional_extent(&mut self) -> Option<f32> {
        self.flag().then(|| self.whole(4, 140))
    }
}

// ---------------------------------------------------------------------------
// serializable tree description
// ---------------------------------------------------------------------------

/// One element's complete logical state.
///
/// Holding every property here - not just the ones that drive layout - is what
/// lets a fresh tree be rebuilt into exactly the state the incremental tree
/// reached, and is why a mutation can be applied to the description and to the
/// live tree from the same source values.
#[derive(Clone, Debug)]
enum Kind {
    Flex {
        axis: Axis,
        gap: f32,
        padding: f32,
        background: Option<Color>,
    },
    Stack {
        padding: f32,
        background: Option<Color>,
    },
    Align {
        alignment: Alignment,
        padding: f32,
    },
    Scroll {
        axis: ScrollAxis,
        offset: LogicalPoint,
    },
    Split {
        axis: Axis,
        ratio: f32,
        divider: Color,
    },
    Frame {
        width: Option<f32>,
        height: Option<f32>,
        min: LogicalSize,
        grow: f32,
    },
    Keys,
    Spin {
        angle: f32,
        scale: f32,
        clip: bool,
    },
    Boxed {
        size: LogicalSize,
        color: Color,
        semantic: bool,
        interactive: bool,
    },
    Text {
        text: String,
        font_size: f32,
        color: Color,
        width: Option<f32>,
    },
    Push {
        label: String,
        size: LogicalSize,
        color: Color,
        pressed: Color,
    },
    Check {
        label: String,
        checked: bool,
        text: Color,
        outline: Color,
        accent: Color,
    },
    Slide {
        label: String,
        value: f32,
        range: (f32, f32),
        step: f32,
        track: Color,
        accent: Color,
    },
    Field {
        label: String,
        value: String,
        text: Color,
        background: Color,
    },
}

impl Kind {
    fn name(&self) -> &'static str {
        match self {
            Self::Flex { .. } => "Flex",
            Self::Stack { .. } => "Stack",
            Self::Align { .. } => "Align",
            Self::Scroll { .. } => "Scroll",
            Self::Split { .. } => "SplitPane",
            Self::Frame { .. } => "Frame",
            Self::Keys => "KeyListener",
            Self::Spin { .. } => "Spin",
            Self::Boxed { .. } => "BoxElement",
            Self::Text { .. } => "Label",
            Self::Push { .. } => "Button",
            Self::Check { .. } => "Checkbox",
            Self::Slide { .. } => "Slider",
            Self::Field { .. } => "TextField",
        }
    }
}

/// Typed handle for one instantiated element.
///
/// [`NodeHandle`] cannot be rebuilt from a [`NodeId`], so the typed mutation
/// API is only reachable through the handle `append` returned. The root has no
/// handle at all, because `UiTree::new` consumes its element.
#[derive(Clone, Copy)]
enum Edit {
    Root,
    Flex(NodeHandle<Flex>),
    Stack(NodeHandle<Stack>),
    Align(NodeHandle<Align>),
    Scroll(NodeHandle<Scroll>),
    Split(NodeHandle<Stack>),
    Frame(NodeHandle<Frame>),
    Keys,
    Spin(NodeHandle<Spin>),
    Boxed(NodeHandle<BoxElement>),
    Text(NodeHandle<Label>),
    Push(NodeHandle<BoxElement>),
    Check(NodeHandle<BoxElement>),
    Slide(NodeHandle<BoxElement>),
    Field(NodeHandle<Label>),
}

impl Edit {
    /// Whether this element exposes any property mutation.
    fn mutable(self) -> bool {
        !matches!(self, Self::Root | Self::Keys)
    }
}

/// One node's binding into a concrete tree.
#[derive(Clone, Copy)]
struct Bound {
    id: NodeId,
    edit: Edit,
}

/// Recursive logical tree description.
#[derive(Clone)]
struct Spec {
    kind: Kind,
    visible: bool,
    enabled: bool,
    children: Vec<Spec>,
    /// Binding into whichever tree this description was last instantiated in.
    /// Overwritten wholesale by [`instantiate`], so a description carrying the
    /// live tree's bindings can be used to build a fresh tree directly.
    bound: Option<Bound>,
}

impl Spec {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            visible: true,
            enabled: true,
            children: Vec::new(),
            bound: None,
        }
    }

    fn id(&self) -> NodeId {
        self.bound.expect("spec was instantiated").id
    }
}

const MAX_NODES: usize = 40;
const MAX_DEPTH: u32 = 5;

fn generate_spec(rng: &mut Rng) -> Spec {
    let mut budget = MAX_NODES - 1;
    // The root is always a clipping `Flex`: `UiTree::new` needs one concrete
    // element type, and a clipping root exercises inherited `world_clip` from
    // the very first level.
    let mut root = Spec::new(Kind::Flex {
        axis: rng.axis(),
        gap: rng.whole(0, 6),
        padding: rng.whole(0, 10),
        background: Some(rng.color()),
    });
    // The root always branches, so every tree has siblings to skip past.
    for _ in 0..2 + rng.below(2) {
        budget = budget.saturating_sub(1);
        root.children.push(generate_node(rng, 1, &mut budget));
    }
    root
}

fn generate_children(rng: &mut Rng, depth: u32, budget: &mut usize, max: u32) -> Vec<Spec> {
    // Containers are usually populated: an empty one contributes no inherited
    // transform or clip to anything, which is the interesting part here. One in
    // ten is still left empty so that degenerate case stays covered.
    let count = if rng.below(12) == 0 {
        0
    } else {
        (1 + rng.below(max) + rng.below(2)).min(max)
    };
    let mut children = Vec::new();
    for _ in 0..count {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        children.push(generate_node(rng, depth, budget));
    }
    children
}

fn generate_node(rng: &mut Rng, depth: u32, budget: &mut usize) -> Spec {
    if depth >= MAX_DEPTH || *budget < 4 || rng.below(10) < 3 {
        return Spec::new(generate_leaf(rng));
    }
    let (kind, max_children) = generate_container(rng);
    let mut node = Spec::new(kind);
    node.children = generate_children(rng, depth + 1, budget, max_children);
    node
}

/// Returns a container kind and how many children it can meaningfully hold.
///
/// Weighted toward containers that hold several children, since a chain of
/// single-child wrappers gives composition no siblings to skip.
fn generate_container(rng: &mut Rng) -> (Kind, u32) {
    match rng.below(13) {
        0..3 => (
            Kind::Flex {
                axis: rng.axis(),
                gap: rng.whole(0, 6),
                padding: rng.whole(0, 8),
                background: rng.flag().then(|| rng.color()),
            },
            3,
        ),
        3..5 => (
            Kind::Stack {
                padding: rng.whole(0, 8),
                background: rng.flag().then(|| rng.color()),
            },
            3,
        ),
        5..7 => (
            Kind::Scroll {
                axis: rng.scroll_axis(),
                offset: LogicalPoint::ZERO,
            },
            2,
        ),
        7..9 => (
            Kind::Split {
                axis: rng.axis(),
                ratio: rng.below(9) as f32 / 10.0 + 0.05,
                divider: rng.color(),
            },
            2,
        ),
        9 => (
            Kind::Align {
                alignment: rng.alignment(),
                padding: rng.whole(0, 8),
            },
            1,
        ),
        10 => (
            Kind::Spin {
                angle: rng.below(12) as f32 * (std::f32::consts::TAU / 12.0),
                scale: 0.5 + rng.below(5) as f32 * 0.25,
                clip: rng.flag(),
            },
            2,
        ),
        11 => (
            Kind::Frame {
                width: rng.optional_extent(),
                height: rng.optional_extent(),
                min: LogicalSize::new(rng.whole(0, 20), rng.whole(0, 20)),
                grow: if rng.flag() { rng.whole(0, 3) } else { 0.0 },
            },
            1,
        ),
        _ => (Kind::Keys, 1),
    }
}

fn generate_leaf(rng: &mut Rng) -> Kind {
    match rng.below(6) {
        0 => Kind::Boxed {
            size: rng.size(),
            color: rng.color(),
            semantic: rng.flag(),
            interactive: rng.below(4) != 0,
        },
        1 => Kind::Text {
            text: rng.word(),
            font_size: rng.whole(8, 24),
            color: rng.color(),
            width: rng.optional_extent(),
        },
        2 => Kind::Push {
            label: rng.word(),
            size: rng.size(),
            color: rng.color(),
            pressed: rng.color(),
        },
        3 => Kind::Check {
            label: rng.word(),
            checked: rng.flag(),
            text: rng.color(),
            outline: rng.color(),
            accent: rng.color(),
        },
        4 => {
            let low = rng.whole(-10, 10);
            Kind::Slide {
                label: rng.word(),
                value: low + rng.whole(0, 20),
                range: (low, low + 20.0),
                step: rng.whole(1, 4),
                track: rng.color(),
                accent: rng.color(),
            }
        }
        _ => Kind::Field {
            label: rng.word(),
            value: rng.word(),
            text: rng.color(),
            background: rng.color(),
        },
    }
}

// ---------------------------------------------------------------------------
// instantiation
// ---------------------------------------------------------------------------

fn semantics(present: bool) -> Option<SemanticData> {
    present.then(|| SemanticData {
        role: SemanticRole::Group,
        label: "boxed".to_owned(),
        ..SemanticData::default()
    })
}

/// Builds a tree from a description, returning a description bound to it.
///
/// Node creation is a strict pre-order walk, so pre-order position identifies
/// the same logical node in every tree built from the same description even
/// though each tree allocates its own [`NodeId`] values.
fn instantiate(spec: &Spec, viewport: LogicalSize) -> (UiTree, Spec) {
    let Kind::Flex {
        axis,
        gap,
        padding,
        background,
    } = spec.kind
    else {
        panic!("the root description must be a Flex");
    };
    let mut ui = UiTree::with_fonts(
        Flex {
            axis,
            gap,
            padding,
            background,
        },
        viewport,
        FontDatabase::empty(),
    );
    let mut bound = spec.clone();
    let root = ui.root();
    bound.bound = Some(Bound {
        id: root,
        edit: Edit::Root,
    });
    for child in &mut bound.children {
        create(&mut ui, root, child);
    }
    // Visibility and enablement come last so the flags land on a complete
    // topology, exactly as they do on the incremental tree.
    apply_flags(&mut ui, &bound);
    (ui, bound)
}

fn create(ui: &mut UiTree, parent: NodeId, node: &mut Spec) {
    create_at(ui, parent, None, node);
}

/// Creates one subtree under `parent`, appended or inserted at a position.
///
/// Only the subtree's own root can be positioned; everything below it is
/// appended, because a freshly generated subtree is already in the order it
/// describes.
fn create_at(ui: &mut UiTree, parent: NodeId, index: Option<usize>, node: &mut Spec) {
    node.bound = Some(place_kind(ui, parent, index, &node.kind));
    let id = node.id();
    for child in &mut node.children {
        create(ui, id, child);
    }
}

/// Binds one element under `parent`, at `index` when one is given.
///
/// The two creation routes share this body so that an inserted element is
/// constructed identically to an appended one, leaving the position as the only
/// difference between them.
fn place_kind(ui: &mut UiTree, parent: NodeId, index: Option<usize>, kind: &Kind) -> Bound {
    macro_rules! bind {
        ($variant:ident, $element:expr) => {{
            let handle = match index {
                Some(index) => ui.insert_child_at(parent, index, $element),
                None => ui.append(parent, $element),
            };
            Bound {
                id: handle.id(),
                edit: Edit::$variant(handle),
            }
        }};
    }
    match kind {
        Kind::Flex {
            axis,
            gap,
            padding,
            background,
        } => bind!(
            Flex,
            Flex {
                axis: *axis,
                gap: *gap,
                padding: *padding,
                background: *background,
            }
        ),
        Kind::Stack {
            padding,
            background,
        } => bind!(
            Stack,
            Stack {
                padding: *padding,
                background: *background,
            }
        ),
        Kind::Align { alignment, padding } => bind!(
            Align,
            Align {
                alignment: *alignment,
                padding: *padding,
            }
        ),
        Kind::Scroll { axis, offset } => {
            let mut element = Scroll::new(*axis);
            element.offset = *offset;
            bind!(Scroll, element)
        }
        Kind::Split {
            axis: _,
            ratio,
            divider,
        } => bind!(
            Split,
            Stack {
                padding: *ratio * 8.0,
                background: Some(*divider)
            }
        ),
        Kind::Frame {
            width,
            height,
            min,
            grow,
        } => bind!(
            Frame,
            Frame {
                width: *width,
                height: *height,
                min: *min,
                max: None,
                grow: *grow,
            }
        ),
        Kind::Keys => {
            let handle = ui.append(parent, Stack::default());
            Bound {
                id: handle.id(),
                edit: Edit::Keys,
            }
        }
        Kind::Spin { angle, scale, clip } => bind!(
            Spin,
            Spin {
                angle: Cell::new(*angle),
                scale: Cell::new(*scale),
                clip: Cell::new(*clip),
            }
        ),
        Kind::Boxed {
            size,
            color,
            semantic,
            interactive,
        } => bind!(
            Boxed,
            BoxElement {
                size: *size,
                color: *color,
                semantics: semantics(*semantic),
                interactive: *interactive,
            }
        ),
        Kind::Text {
            text,
            font_size,
            color,
            width,
        } => {
            let mut element = Label::new(text.clone())
                .with_font_size(*font_size)
                .with_color(*color);
            element.set_width(*width);
            bind!(Text, element)
        }
        Kind::Push {
            label,
            size,
            color,
            pressed: _,
        } => bind!(
            Push,
            BoxElement {
                size: *size,
                color: *color,
                semantics: Some(SemanticData {
                    role: SemanticRole::Button,
                    label: label.clone(),
                    ..SemanticData::default()
                }),
                interactive: true
            }
        ),
        Kind::Check {
            label,
            checked,
            text: _,
            outline,
            accent,
        } => bind!(
            Check,
            BoxElement {
                size: LogicalSize::new(80.0, 20.0),
                color: if *checked { *accent } else { *outline },
                semantics: Some(SemanticData {
                    role: SemanticRole::Checkbox,
                    label: label.clone(),
                    ..SemanticData::default()
                }),
                interactive: true
            }
        ),
        Kind::Slide {
            label,
            value,
            range,
            step,
            track,
            accent,
        } => bind!(
            Slide,
            BoxElement {
                size: LogicalSize::new(100.0 + *step, 20.0),
                color: if *value > range.0 { *accent } else { *track },
                semantics: Some(SemanticData {
                    role: SemanticRole::Slider,
                    label: label.clone(),
                    ..SemanticData::default()
                }),
                interactive: true
            }
        ),
        Kind::Field {
            label,
            value,
            text,
            background: _,
        } => {
            bind!(
                Field,
                Label::new(format!("{label}: {value}")).with_color(*text)
            )
        }
    }
}

fn apply_flags(ui: &mut UiTree, node: &Spec) {
    let id = node.id();
    if !node.enabled {
        ui.set_enabled(id, false);
    }
    if !node.visible {
        ui.set_visible(id, false);
    }
    for child in &node.children {
        apply_flags(ui, child);
    }
}

// ---------------------------------------------------------------------------
// description navigation
// ---------------------------------------------------------------------------

fn count(node: &Spec) -> usize {
    1 + node.children.iter().map(count).sum::<usize>()
}

fn flatten<'a>(node: &'a Spec, output: &mut Vec<&'a Spec>) {
    output.push(node);
    for child in &node.children {
        flatten(child, output);
    }
}

fn preorder(node: &Spec) -> Vec<&Spec> {
    let mut output = Vec::new();
    flatten(node, &mut output);
    output
}

/// Returns the node at `target` in pre-order, counting from `counter`.
fn nth_mut<'a>(node: &'a mut Spec, target: usize, counter: &mut usize) -> Option<&'a mut Spec> {
    if *counter == target {
        return Some(node);
    }
    *counter += 1;
    for child in &mut node.children {
        if let Some(found) = nth_mut(child, target, counter) {
            return Some(found);
        }
    }
    None
}

fn node_at_mut(root: &mut Spec, target: usize) -> Option<&mut Spec> {
    nth_mut(root, target, &mut 0)
}

/// Detaches the node at pre-order position `target`, which must not be the root.
fn detach(node: &mut Spec, target: usize, counter: &mut usize) -> Option<Spec> {
    *counter += 1;
    for position in 0..node.children.len() {
        if *counter == target {
            return Some(node.children.remove(position));
        }
        if let Some(found) = detach(&mut node.children[position], target, counter) {
            return Some(found);
        }
    }
    None
}

/// Returns the pre-order position of `id`, which must be live.
fn position_of(root: &Spec, id: NodeId) -> usize {
    preorder(root)
        .into_iter()
        .position(|node| node.id() == id)
        .expect("live identity")
}

/// Returns every identity in the subtree rooted at `node`, itself included.
fn subtree_ids(node: &Spec) -> Vec<NodeId> {
    preorder(node).into_iter().map(Spec::id).collect()
}

/// Maps each live identity to its pre-order position.
fn positions(root: &Spec) -> HashMap<NodeId, usize> {
    preorder(root)
        .into_iter()
        .enumerate()
        .map(|(index, node)| (node.id(), index))
        .collect()
}

/// Pre-order positions of nodes painted this frame: self and every ancestor
/// visible. `collect_fragments` walks exactly this set, in this order.
fn painted(root: &Spec) -> Vec<usize> {
    let mut output = Vec::new();
    let mut index = 0;
    collect_painted(root, true, &mut index, &mut output);
    output
}

fn collect_painted(node: &Spec, ancestors: bool, index: &mut usize, output: &mut Vec<usize>) {
    let visible = ancestors && node.visible;
    if visible {
        output.push(*index);
    }
    *index += 1;
    for child in &node.children {
        collect_painted(child, visible, index, output);
    }
}

/// Pre-order positions whose element sits inside a `Scroll`, itself included.
fn scroll_scope(root: &Spec) -> HashSet<NodeId> {
    let mut output = HashSet::new();
    collect_scroll_scope(root, false, &mut output);
    output
}

fn collect_scroll_scope(node: &Spec, inside: bool, output: &mut HashSet<NodeId>) {
    let inside = inside || matches!(node.kind, Kind::Scroll { .. });
    if inside {
        output.insert(node.id());
    }
    for child in &node.children {
        collect_scroll_scope(child, inside, output);
    }
}

// ---------------------------------------------------------------------------
// property mutation
// ---------------------------------------------------------------------------

/// Rerolls every property, including the ones that resize the element.
fn randomize_layout(rng: &mut Rng, kind: &mut Kind) {
    match kind {
        Kind::Flex {
            axis,
            gap,
            padding,
            background,
        } => {
            *axis = rng.axis();
            *gap = rng.whole(0, 8);
            *padding = rng.whole(0, 10);
            *background = rng.flag().then(|| rng.color());
        }
        Kind::Stack {
            padding,
            background,
        } => {
            *padding = rng.whole(0, 10);
            *background = rng.flag().then(|| rng.color());
        }
        Kind::Align { alignment, padding } => {
            *alignment = rng.alignment();
            *padding = rng.whole(0, 10);
        }
        Kind::Scroll { axis, offset } => {
            *axis = rng.scroll_axis();
            *offset = LogicalPoint::new(rng.whole(0, 60), rng.whole(0, 60));
        }
        Kind::Split { axis, ratio, .. } => {
            *axis = rng.axis();
            *ratio = rng.below(9) as f32 / 10.0 + 0.05;
        }
        Kind::Frame {
            width,
            height,
            min,
            grow,
        } => {
            *width = rng.optional_extent();
            *height = rng.optional_extent();
            *min = LogicalSize::new(rng.whole(0, 30), rng.whole(0, 30));
            *grow = if rng.flag() { rng.whole(0, 3) } else { 0.0 };
        }
        Kind::Keys => {}
        Kind::Spin { angle, scale, clip } => {
            *angle = rng.below(12) as f32 * (std::f32::consts::TAU / 12.0);
            *scale = 0.5 + rng.below(5) as f32 * 0.25;
            *clip = rng.flag();
        }
        Kind::Boxed {
            size,
            color,
            semantic,
            interactive,
        } => {
            *size = rng.size();
            *color = rng.color();
            *semantic = rng.flag();
            *interactive = rng.below(4) != 0;
        }
        Kind::Text {
            text,
            font_size,
            color,
            width,
        } => {
            *text = rng.word();
            *font_size = rng.whole(8, 24);
            *color = rng.color();
            *width = rng.optional_extent();
        }
        Kind::Push {
            label,
            size,
            color,
            pressed,
        } => {
            *label = rng.word();
            *size = rng.size();
            *color = rng.color();
            *pressed = rng.color();
        }
        Kind::Check {
            label,
            checked,
            text,
            outline,
            accent,
        } => {
            *label = rng.word();
            *checked = rng.flag();
            *text = rng.color();
            *outline = rng.color();
            *accent = rng.color();
        }
        Kind::Slide {
            label,
            value,
            range,
            step,
            track,
            accent,
        } => {
            let low = rng.whole(-10, 10);
            *label = rng.word();
            *range = (low, low + 20.0);
            *value = low + rng.whole(0, 20);
            *step = rng.whole(1, 4);
            *track = rng.color();
            *accent = rng.color();
        }
        Kind::Field {
            label,
            value,
            text,
            background,
        } => {
            *label = rng.word();
            *value = rng.word();
            *text = rng.color();
            *background = rng.color();
        }
    }
}

/// Rerolls only properties that cannot move geometry, so the element may
/// honestly report [`Invalidation::PAINT`] alone.
fn randomize_paint(rng: &mut Rng, kind: &mut Kind) -> bool {
    match kind {
        Kind::Flex { background, .. } | Kind::Stack { background, .. } => {
            *background = Some(rng.color());
            true
        }
        Kind::Split { divider, .. } => {
            *divider = rng.color();
            true
        }
        Kind::Boxed { color, .. } => {
            *color = rng.color();
            true
        }
        Kind::Check { accent, .. } => {
            *accent = rng.color();
            true
        }
        Kind::Slide { track, .. } => {
            *track = rng.color();
            true
        }
        _ => false,
    }
}

/// Pushes every property into the live element, declaring the work that
/// property honestly needs: [`Invalidation::LAYOUT_ALL`] for everything the
/// typed mutation API covers, and compose-without-layout for `Spin`, whose
/// transform and clip cannot change its own size or its children's constraints.
fn push_properties(ui: &mut UiTree, bound: Bound, kind: &Kind) {
    match (bound.edit, kind) {
        (Edit::Spin(handle), Kind::Spin { angle, scale, clip }) => {
            let element = ui.element(handle);
            let angle_changed = element.angle.replace(*angle) != *angle;
            let scale_changed = element.scale.replace(*scale) != *scale;
            let clip_changed = element.clip.replace(*clip) != *clip;
            let changed = angle_changed || scale_changed || clip_changed;
            if changed {
                ui.edit(handle).mark_composition_changed();
            }
        }
        (
            Edit::Flex(handle),
            Kind::Flex {
                axis,
                gap,
                padding,
                background,
            },
        ) => ui.edit(handle).set_flex(*axis, *gap, *padding, *background),
        (
            Edit::Stack(handle),
            Kind::Stack {
                padding,
                background,
            },
        ) => ui.edit(handle).set_stack(*padding, *background),
        (Edit::Align(handle), Kind::Align { alignment, padding }) => {
            ui.align_mut(handle).set_align(*alignment, *padding);
        }
        (Edit::Scroll(handle), Kind::Scroll { axis, offset }) => {
            ui.edit(handle).set_scroll(*axis, *offset)
        }
        (Edit::Split(handle), Kind::Split { ratio, divider, .. }) => {
            ui.stack_mut(handle).set_stack(*ratio * 8.0, Some(*divider));
        }
        (
            Edit::Frame(handle),
            Kind::Frame {
                width,
                height,
                min,
                grow,
            },
        ) => ui
            .edit(handle)
            .set_frame(*width, *height, *min, None, *grow),
        (
            Edit::Boxed(handle),
            Kind::Boxed {
                size,
                color,
                semantic,
                interactive,
            },
        ) => ui
            .edit(handle)
            .set_box(*size, *color, semantics(*semantic), *interactive),
        (
            Edit::Text(handle),
            Kind::Text {
                text,
                font_size,
                color,
                width,
            },
        ) => ui
            .edit(handle)
            .set_content(text.clone(), *font_size, *color, *width),
        (
            Edit::Push(handle),
            Kind::Push {
                label,
                size,
                color,
                pressed: _,
            },
        ) => ui.box_mut(handle).set_box(
            *size,
            *color,
            Some(SemanticData {
                role: SemanticRole::Button,
                label: label.clone(),
                ..SemanticData::default()
            }),
            true,
        ),
        (
            Edit::Check(handle),
            Kind::Check {
                label,
                checked,
                text: _,
                outline,
                accent,
            },
        ) => ui.box_mut(handle).set_box(
            LogicalSize::new(80.0, 20.0),
            if *checked { *accent } else { *outline },
            Some(SemanticData {
                role: SemanticRole::Checkbox,
                label: label.clone(),
                ..SemanticData::default()
            }),
            true,
        ),
        (
            Edit::Slide(handle),
            Kind::Slide {
                label,
                value,
                range,
                step,
                track,
                accent,
            },
        ) => ui.box_mut(handle).set_box(
            LogicalSize::new(100.0 + *step, 20.0),
            if *value > range.0 { *accent } else { *track },
            Some(SemanticData {
                role: SemanticRole::Slider,
                label: label.clone(),
                ..SemanticData::default()
            }),
            true,
        ),
        (
            Edit::Field(handle),
            Kind::Field {
                label,
                value,
                text,
                background: _,
            },
        ) => ui
            .label_mut(handle)
            .set_content(format!("{label}: {value}"), 14.0, *text, None),
        _ => panic!("edit handle and description kind disagree"),
    }
}

/// Pushes only paint properties, declaring [`Invalidation::PAINT`] alone.
fn push_paint(ui: &mut UiTree, bound: Bound, kind: &Kind) {
    match (bound.edit, kind) {
        (Edit::Flex(handle), Kind::Flex { background, .. }) => {
            ui.flex_mut(handle).set_background(*background);
        }
        (Edit::Stack(handle), Kind::Stack { background, .. }) => {
            ui.stack_mut(handle).set_background(*background);
        }
        (Edit::Split(handle), Kind::Split { divider, .. }) => {
            ui.stack_mut(handle).set_background(Some(*divider));
        }
        (Edit::Boxed(handle), Kind::Boxed { color, .. }) => {
            ui.box_mut(handle).set_color(*color);
        }
        (Edit::Check(handle), Kind::Check { accent, .. }) => {
            ui.box_mut(handle).set_color(*accent);
        }
        (Edit::Slide(handle), Kind::Slide { track, .. }) => {
            ui.box_mut(handle).set_color(*track);
        }
        _ => panic!("paint mutation reached an element without paint-only state"),
    }
}

/// Copies scroll offsets the runtime clamped back into the description, so a
/// fresh tree starts from the offsets the live tree actually holds.
fn resync(ui: &UiTree, node: &mut Spec) {
    if let (Some(bound), Kind::Scroll { offset, .. }) = (node.bound, &mut node.kind)
        && let Edit::Scroll(handle) = bound.edit
    {
        *offset = ui.element(handle).offset;
    }
    for child in &mut node.children {
        resync(ui, child);
    }
}

// ---------------------------------------------------------------------------
// mutations
// ---------------------------------------------------------------------------

/// Applies one random mutation to the live tree and its description, returning
/// a description of what it did for failure reporting.
fn mutate(rng: &mut Rng, ui: &mut UiTree, state: &mut Spec, viewport: &mut LogicalSize) -> String {
    let total = count(state);
    match rng.below(100) {
        0..14 if total > 1 => {
            let index = 1 + rng.below((total - 1) as u32) as usize;
            let node = node_at_mut(state, index).expect("node in range");
            let visible = !node.visible;
            node.visible = visible;
            let id = node.id();
            let name = node.kind.name();
            ui.set_visible(id, visible);
            format!("set_visible(#{index} {name}, {visible})")
        }
        14..22 if total > 1 => {
            let index = 1 + rng.below((total - 1) as u32) as usize;
            let node = node_at_mut(state, index).expect("node in range");
            let enabled = !node.enabled;
            node.enabled = enabled;
            let id = node.id();
            let name = node.kind.name();
            ui.set_enabled(id, enabled);
            format!("set_enabled(#{index} {name}, {enabled})")
        }
        22..30 => {
            let next = LogicalSize::new(rng.whole(140, 480), rng.whole(120, 360));
            *viewport = next;
            ui.set_viewport(next);
            format!("set_viewport({} x {})", next.width, next.height)
        }
        30..40 => {
            let candidates = preorder(state)
                .iter()
                .enumerate()
                .filter(|(_, node)| node.children.len() > 1)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if candidates.is_empty() {
                return mutate_property(rng, ui, state, viewport);
            }
            let index = candidates[rng.below(candidates.len() as u32) as usize];
            let node = node_at_mut(state, index).expect("node in range");
            let len = node.children.len();
            let first = rng.below(len as u32) as usize;
            let second = rng.below(len as u32) as usize;
            if rng.flag() {
                node.children.rotate_left(1);
            } else {
                node.children.swap(first, second);
            }
            let parent = node.id();
            let children = node.children.iter().map(Spec::id).collect::<Vec<_>>();
            ui.set_children(parent, &children);
            format!("set_children(#{index}) -> {len} children reordered")
        }
        40..46 if total < MAX_NODES + 8 => {
            // Inserting into the middle of a child list, which is what a keyed
            // reconciler emits when one row appears above the others. The fresh
            // tree builds that order from scratch, so this is the mutation that
            // catches an insertion the passes treat as an append.
            let index = rng.below(total as u32) as usize;
            let mut budget = 2;
            let mut child = generate_node(rng, MAX_DEPTH - 1, &mut budget);
            let node = node_at_mut(state, index).expect("node in range");
            let parent = node.id();
            let at = rng.below(node.children.len() as u32 + 1) as usize;
            create_at(ui, parent, Some(at), &mut child);
            let name = child.kind.name();
            node.children.insert(at, child);
            format!("insert_child_at(#{index}, {at}, {name})")
        }
        46..52 => {
            let candidates = preorder(state)
                .iter()
                .enumerate()
                .filter(|(_, node)| node.children.len() > 1)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if candidates.is_empty() {
                return mutate_property(rng, ui, state, viewport);
            }
            let index = candidates[rng.below(candidates.len() as u32) as usize];
            let node = node_at_mut(state, index).expect("node in range");
            let len = node.children.len();
            let from = rng.below(len as u32) as usize;
            // `index` counts positions in the resulting list and the engine
            // clamps it to the last one, so the mirror clamps identically rather
            // than restricting the draw.
            let to = (rng.below(len as u32 + 2) as usize).min(len - 1);
            let parent = node.id();
            let child = node.children[from].id();
            let moved = node.children.remove(from);
            node.children.insert(to, moved);
            ui.move_child(parent, child, to);
            format!("move_child(#{index}, {from} -> {to} of {len})")
        }
        52..58 if total > 3 => {
            // The strongest topology mutation: a subtree keeps its identities,
            // fragments, semantics, and cached constraints while inheriting a
            // different transform and clip. Nothing about that follows from the
            // two parents relaying out, so it is exactly the case a pruning pass
            // can miss.
            let index = 1 + rng.below((total - 1) as u32) as usize;
            let subject = preorder(state)[index];
            let subject_id = subject.id();
            let forbidden = subtree_ids(subject);
            let hosts = preorder(state)
                .into_iter()
                .map(Spec::id)
                .filter(|id| !forbidden.contains(id))
                .collect::<Vec<_>>();
            if hosts.is_empty() {
                return mutate_property(rng, ui, state, viewport);
            }
            let parent = hosts[rng.below(hosts.len() as u32) as usize];
            let moved = detach(state, index, &mut 0).expect("node in range");
            let name = moved.kind.name();
            // Resolved after the detach, because that is the child count the
            // engine clamps against and the position the mirror must insert at.
            let host = node_at_mut(state, position_of(state, parent)).expect("host in range");
            let at = rng.below(host.children.len() as u32 + 1) as usize;
            host.children.insert(at, moved);
            ui.reparent(subject_id, parent, at);
            format!("reparent(#{index} {name} -> {parent:?}[{at}])")
        }
        58..64 if total < MAX_NODES + 8 => {
            // Any container may grow, including past the arity it documents.
            // `Align` and `KeyListener` lay out only their first child and
            // `SplitPane` only its first two, so this is what generates children
            // the pass never measures - and the tree owes those the same zero
            // geometry a fresh build gives them.
            let index = rng.below(total as u32) as usize;
            let mut budget = 2;
            let mut child = generate_node(rng, MAX_DEPTH - 1, &mut budget);
            let node = node_at_mut(state, index).expect("node in range");
            let parent = node.id();
            create(ui, parent, &mut child);
            let name = child.kind.name();
            node.children.push(child);
            format!("append(#{index}, {name})")
        }
        64..70 if total > 3 => {
            let index = 1 + rng.below((total - 1) as u32) as usize;
            let removed = detach(state, index, &mut 0).expect("node in range");
            let name = removed.kind.name();
            ui.remove(removed.id());
            format!("remove(#{index} {name})")
        }
        70..88 => mutate_property(rng, ui, state, viewport),
        _ => {
            let scope = scroll_scope(state);
            let mut position = LogicalPoint::new(
                rng.whole(0, viewport.width as i32),
                rng.whole(0, viewport.height as i32),
            );
            if !scope.is_empty() {
                // Aim the wheel at a scrollable subtree so the mutation
                // actually moves content instead of being swallowed.
                for _ in 0..24 {
                    if ui.hit_test(position).is_some_and(|id| scope.contains(&id)) {
                        break;
                    }
                    position = LogicalPoint::new(
                        rng.whole(0, viewport.width as i32),
                        rng.whole(0, viewport.height as i32),
                    );
                }
            }
            let delta = LogicalPoint::new(rng.whole(-60, 60), rng.whole(-60, 60));
            ui.dispatch(UiInput::PointerWheel { position, delta });
            format!(
                "wheel(at {},{} by {},{})",
                position.x, position.y, delta.x, delta.y
            )
        }
    }
}

fn mutate_property(
    rng: &mut Rng,
    ui: &mut UiTree,
    state: &mut Spec,
    viewport: &mut LogicalSize,
) -> String {
    let total = count(state);
    let candidates = preorder(state)
        .iter()
        .enumerate()
        .filter(|(_, node)| node.bound.is_some_and(|bound| bound.edit.mutable()))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        // Only the root is left, and the root element is unreachable: `UiTree`
        // hands out no handle for it. Resize instead so the step still does
        // something.
        let next = LogicalSize::new(rng.whole(140, 480), rng.whole(120, 360));
        *viewport = next;
        ui.set_viewport(next);
        return format!(
            "set_viewport({} x {}) [no editable node]",
            next.width, next.height
        );
    }
    let index = candidates[rng.below(candidates.len() as u32) as usize];
    let node = node_at_mut(state, index).expect("node in range");
    let bound = node.bound.expect("bound");
    let paint_only = rng.below(3) == 0 && randomize_paint(rng, &mut node.kind);
    if !paint_only {
        randomize_layout(rng, &mut node.kind);
    }
    let kind = node.kind.clone();
    let name = kind.name();
    if paint_only {
        push_paint(ui, bound, &kind);
        format!("paint-only property on #{index} {name} (of {total})")
    } else {
        push_properties(ui, bound, &kind);
        format!("layout property on #{index} {name} (of {total})")
    }
}

// ---------------------------------------------------------------------------
// comparison
// ---------------------------------------------------------------------------

/// Semantic node projected onto pre-order positions, since the two trees hand
/// out different identities for the same logical node.
#[derive(Debug, PartialEq)]
struct SemanticRow {
    index: usize,
    parent: Option<usize>,
    bounds: LogicalRect,
    data: SemanticData,
    focusable: bool,
    focused: bool,
    enabled: bool,
    actions: Vec<SemanticActionKind>,
}

fn semantic_rows(ui: &UiTree, positions: &HashMap<NodeId, usize>, label: &str) -> Vec<SemanticRow> {
    let mut rows = ui
        .semantic_snapshot()
        .into_iter()
        .map(|node| SemanticRow {
            index: *positions
                .get(&node.id)
                .unwrap_or_else(|| panic!("{label}: semantic node outside the description")),
            parent: node
                .parent
                .map(|parent| *positions.get(&parent).expect("semantic parent")),
            bounds: node.bounds,
            data: node.data,
            focusable: node.focusable,
            focused: node.focused,
            enabled: node.enabled,
            actions: node.actions,
        })
        .collect::<Vec<_>>();
    // `semantic_snapshot` walks slot order, which differs between the two trees
    // once removal has recycled slots.
    rows.sort_by_key(|row| row.index);
    rows
}

/// Points to hit-test: a viewport grid, plus the exact edges of every clip
/// rectangle and every fragment origin, offset a half pixel either way.
fn probe_points(ui: &UiTree, viewport: LogicalSize) -> Vec<LogicalPoint> {
    const GRID_X: u32 = 9;
    const GRID_Y: u32 = 7;
    const EDGE: f32 = 0.5;
    const LIMIT: usize = 420;

    let mut seen = HashSet::new();
    let mut points = Vec::new();
    {
        let mut push = |x: f32, y: f32| {
            if points.len() < LIMIT && seen.insert((x.to_bits(), y.to_bits())) {
                points.push(LogicalPoint::new(x, y));
            }
        };
        for row in 0..GRID_Y {
            for column in 0..GRID_X {
                push(
                    (column as f32 + 0.5) * viewport.width / GRID_X as f32,
                    (row as f32 + 0.5) * viewport.height / GRID_Y as f32,
                );
            }
        }
        for instance in ui.scene().fragments() {
            let origin = instance.transform.translation;
            push(origin.x + EDGE, origin.y + EDGE);
            push(origin.x - EDGE, origin.y - EDGE);
            if let Some(clip) = instance.clip {
                let (left, top) = (clip.origin.x, clip.origin.y);
                let (right, bottom) = (clip.max_x(), clip.max_y());
                let middle_x = left + clip.size.width * 0.5;
                let middle_y = top + clip.size.height * 0.5;
                for (x, y) in [
                    (left + EDGE, top + EDGE),
                    (left - EDGE, top - EDGE),
                    (right - EDGE, bottom - EDGE),
                    (right + EDGE, bottom + EDGE),
                    (middle_x, top + EDGE),
                    (middle_x, top - EDGE),
                    (middle_x, bottom - EDGE),
                    (middle_x, bottom + EDGE),
                    (left + EDGE, middle_y),
                    (left - EDGE, middle_y),
                    (right - EDGE, middle_y),
                    (right + EDGE, middle_y),
                ] {
                    push(x, y);
                }
            }
        }
    }
    points
}

/// Asserts the incrementally updated tree agrees with a fresh tree in the same
/// logical state, on everything composition feeds.
///
/// That is every public surface the composed values reach. `UiTree` exposes no
/// per-node geometry accessor, so each composed field is covered through what
/// consumes it: `world_transform` and `world_clip` through the display-list
/// instances, `world_inverse` and `subtree_bounds` through hit testing - a
/// stale `subtree_bounds` prunes the wrong subtree, a stale `world_inverse`
/// localizes the wrong point - and `world_bounds` through the accessibility
/// snapshot, which reports it verbatim.
fn assert_equivalent(context: &str, live: &mut UiTree, state: &Spec, viewport: LogicalSize) {
    let (mut fresh, fresh_state) = instantiate(state, viewport);
    fresh.update_passes();

    let expected_painted = painted(state);
    let kinds = preorder(state)
        .into_iter()
        .map(|node| node.kind.name())
        .collect::<Vec<_>>();

    let live_fragments = live.scene().fragments().to_vec();
    let fresh_fragments = fresh.scene().fragments().to_vec();
    assert_eq!(
        live_fragments.len(),
        fresh_fragments.len(),
        "{context}: fragment count diverged - incremental {} vs fresh {} \
         (expected {} painted nodes)",
        live_fragments.len(),
        fresh_fragments.len(),
        expected_painted.len(),
    );
    assert_eq!(
        live_fragments.len(),
        expected_painted.len(),
        "{context}: the incremental tree painted {} fragments but {} nodes have \
         an unbroken visible ancestor chain",
        live_fragments.len(),
        expected_painted.len(),
    );
    for (position, (live_instance, fresh_instance)) in
        live_fragments.iter().zip(&fresh_fragments).enumerate()
    {
        let node = expected_painted[position];
        assert!(
            live_instance.transform == fresh_instance.transform
                && live_instance.clip == fresh_instance.clip
                && live_instance.opacity == fresh_instance.opacity,
            "{context}: fragment {position} (node #{node} {kind}) diverged\n  \
             incremental: transform {:?} clip {:?} opacity {}\n  \
             fresh:       transform {:?} clip {:?} opacity {}",
            live_instance.transform,
            live_instance.clip,
            live_instance.opacity,
            fresh_instance.transform,
            fresh_instance.clip,
            fresh_instance.opacity,
            kind = kinds[node],
        );
    }

    let live_positions = positions(state);
    let fresh_positions = positions(&fresh_state);
    for point in probe_points(live, viewport) {
        let live_hit = live.hit_test(point).map(|id| {
            *live_positions
                .get(&id)
                .expect("incremental hit outside the description")
        });
        let fresh_hit = fresh.hit_test(point).map(|id| {
            *fresh_positions
                .get(&id)
                .expect("fresh hit outside the description")
        });
        assert_eq!(
            live_hit,
            fresh_hit,
            "{context}: hit_test({}, {}) diverged - incremental {} vs fresh {}",
            point.x,
            point.y,
            describe_hit(live_hit, &kinds),
            describe_hit(fresh_hit, &kinds),
        );
    }

    let live_rows = semantic_rows(live, &live_positions, "incremental");
    let fresh_rows = semantic_rows(&fresh, &fresh_positions, "fresh");
    assert_eq!(
        live_rows.len(),
        fresh_rows.len(),
        "{context}: semantic node count diverged - incremental {:?} vs fresh {:?}",
        live_rows.iter().map(|row| row.index).collect::<Vec<_>>(),
        fresh_rows.iter().map(|row| row.index).collect::<Vec<_>>(),
    );
    for (live_row, fresh_row) in live_rows.iter().zip(&fresh_rows) {
        assert_eq!(
            live_row,
            fresh_row,
            "{context}: semantics diverged for node #{} {}\n  topology: {}",
            live_row.index,
            kinds[live_row.index],
            // Which element hosts which is the first thing a geometry divergence
            // needs and the one thing the two rows do not say: whether the
            // reported parent measures its children at all is usually the whole
            // explanation.
            describe_topology(state),
        );
    }
}

/// One-line pre-order summary of who parents whom.
fn describe_topology(state: &Spec) -> String {
    preorder(state)
        .iter()
        .enumerate()
        .map(|(index, node)| {
            format!(
                "#{index}={}({} kids)",
                node.kind.name(),
                node.children.len()
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn describe_hit(hit: Option<usize>, kinds: &[&'static str]) -> String {
    match hit {
        None => "nothing".to_owned(),
        Some(index) => format!("#{index} {}", kinds[index]),
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

/// Property-style differential test.
///
/// Sizing rationale: 192 seeds x 24 mutations is ~1.6s in a debug build, which
/// buys 4608 mutation steps, every element type, every mutation kind, and -
/// printed below - thousands of genuinely skipped subtrees. Trees stay at or
/// just under the 40-node budget so a reported failure is small enough to read.
/// A 1500 x 40 sweep (60000 steps, 117896 skipped subtrees) also passes, so the
/// CI size is a runtime choice rather than the limit of what was checked.
///
/// Every failure names the seed and the mutation index, and the sequence is a
/// pure function of the seed, so a failure replays from the seed alone.
#[test]
fn differential_compose_equivalence() {
    const SEEDS: u64 = 192;
    const STEPS: usize = 24;

    let mut totals = PassStats::default();
    for seed in 0..SEEDS {
        let mut rng = Rng::new(seed + 1);
        let mut viewport = LogicalSize::new(rng.whole(200, 460), rng.whole(160, 340));
        let spec = generate_spec(&mut rng);
        let (mut live, mut state) = instantiate(&spec, viewport);
        live.update_passes();
        resync(&live, &mut state);
        assert_equivalent(
            &format!("seed {seed} / step 0 (initial build)"),
            &mut live,
            &state,
            viewport,
        );
        for step in 1..=STEPS {
            let description = mutate(&mut rng, &mut live, &mut state, &mut viewport);
            let stats = live.update_passes().stats;
            totals.visited_compose_nodes += stats.visited_compose_nodes;
            totals.compose_skipped_subtrees += stats.compose_skipped_subtrees;
            resync(&live, &mut state);
            assert_equivalent(
                &format!("seed {seed} / step {step} ({description})"),
                &mut live,
                &state,
                viewport,
            );
        }
    }

    // A differential test that never skipped anything would pass even if the
    // incremental pass had silently degraded into a full walk, so prove the
    // optimization ran.
    assert!(
        totals.compose_skipped_subtrees > 0,
        "no subtree was ever skipped, so nothing proved the incremental path ran"
    );
    eprintln!(
        "compose: {} nodes visited, {} subtrees skipped across {SEEDS} seeds x {STEPS} mutations",
        totals.visited_compose_nodes, totals.compose_skipped_subtrees,
    );
}

/// Hiding a node clears its compose bits, so showing it again must still
/// recompose the whole subtree.
///
/// Root-ward invalidation stops at the first ancestor already carrying the
/// bits it would add. A hidden node that kept its compose bits would therefore
/// swallow the propagation from `set_visible`, and its subtree - whose cached
/// geometry was zeroed while hidden - would never be walked again.
#[test]
fn hidden_subtree_recomposes_when_shown_again() {
    let mut interior = Spec::new(Kind::Scroll {
        axis: ScrollAxis::Vertical,
        offset: LogicalPoint::ZERO,
    });
    let mut inner = Spec::new(Kind::Flex {
        axis: Axis::Vertical,
        gap: 4.0,
        padding: 3.0,
        background: None,
    });
    inner.children.push(Spec::new(Kind::Boxed {
        size: LogicalSize::new(30.0, 20.0),
        color: Color::WHITE,
        semantic: true,
        interactive: true,
    }));
    inner.children.push(Spec::new(Kind::Push {
        label: "inner".to_owned(),
        size: LogicalSize::new(60.0, 24.0),
        color: Color::BLUE,
        pressed: Color::BLACK,
    }));
    interior.children.push(inner);

    let mut spec = Spec::new(Kind::Flex {
        axis: Axis::Vertical,
        gap: 5.0,
        padding: 7.0,
        background: Some(Color::BLACK),
    });
    spec.children.push(Spec::new(Kind::Boxed {
        size: LogicalSize::new(40.0, 15.0),
        color: Color::BLUE,
        semantic: true,
        interactive: true,
    }));
    spec.children.push(interior);
    spec.children.push(Spec::new(Kind::Check {
        label: "after".to_owned(),
        checked: true,
        text: Color::WHITE,
        outline: Color::WHITE,
        accent: Color::BLUE,
    }));

    let viewport = LogicalSize::new(320.0, 240.0);
    let (mut ui, mut state) = instantiate(&spec, viewport);
    ui.update_passes();

    // #1 is the leading box, #2 the interior Scroll, #3 the Flex inside it.
    for hidden in [2usize, 3] {
        let node = node_at_mut(&mut state, hidden).expect("node in range");
        node.visible = false;
        let id = node.id();
        ui.set_visible(id, false);
        ui.update_passes();
        assert_equivalent(
            &format!("after hiding #{hidden}"),
            &mut ui,
            &state,
            viewport,
        );
    }

    // Show the inner one first: it stays effectively invisible because its
    // parent is still hidden, which is the nested case that must not lose the
    // pending compose work.
    for shown in [3usize, 2] {
        let node = node_at_mut(&mut state, shown).expect("node in range");
        node.visible = true;
        let id = node.id();
        ui.set_visible(id, true);
        ui.update_passes();
        assert_equivalent(
            &format!("after showing #{shown}"),
            &mut ui,
            &state,
            viewport,
        );
    }
}

/// Resizing one child of a `Stack` must not recompose its siblings.
///
/// A suite that only proved correctness would still pass with the skip rule
/// disabled, leaving the optimization silently dead, so pin the counters.
#[test]
fn moving_a_node_does_not_recompose_its_siblings() {
    let viewport = LogicalSize::new(300.0, 200.0);
    let mut ui = UiTree::with_fonts(
        Stack {
            padding: 0.0,
            background: None,
        },
        viewport,
        FontDatabase::empty(),
    );
    let root = ui.root();

    // `Stack` overlays every child at the same origin, so resizing one cannot
    // move another. Sibling subtrees therefore inherit an unchanged transform
    // and clip and must be skipped whole.
    let moved = ui.append(
        root,
        Frame {
            width: Some(50.0),
            height: Some(50.0),
            ..Frame::default()
        },
    );
    ui.append(
        moved.id(),
        BoxElement::new(LogicalSize::new(10.0, 10.0), Color::WHITE),
    );

    let sibling = ui.append(
        root,
        Frame {
            width: Some(70.0),
            height: Some(70.0),
            ..Frame::default()
        },
    );
    let nested = ui.append(
        sibling.id(),
        Flex {
            axis: Axis::Vertical,
            ..Flex::default()
        },
    );
    ui.append(
        nested.id(),
        BoxElement::new(LogicalSize::new(12.0, 12.0), Color::BLUE),
    );
    ui.append(nested.id(), Label::new("sibling"));

    ui.update_passes();
    let settled = ui.update_passes().stats;
    assert_eq!(
        settled.visited_compose_nodes, 0,
        "a clean update must not compose at all"
    );

    ui.edit(moved)
        .set_frame(Some(90.0), Some(90.0), LogicalSize::ZERO, None, 0.0);
    let stats = ui.update_passes().stats;

    assert_eq!(
        stats.compose_skipped_subtrees, 1,
        "the untouched sibling subtree must be skipped at its root"
    );
    assert_eq!(
        stats.visited_compose_nodes, 3,
        "only the Stack, the resized Frame, and its box may be recomposed, \
         leaving the sibling's four nodes untouched"
    );
}

/// A child its parent stops laying out must not keep its old geometry.
///
/// `layout_node` is only reached through a parent's `layout_child` calls, so
/// before the pass zeroed unmeasured children, one that stopped being measured
/// kept its last `size` and `offset` forever and went on painting,
/// hit-testing, and reporting accessible bounds at geometry that corresponded
/// to nothing, while the same tree built from scratch gave it a zero size.
///
/// `Align` reaches it with two children: it lays out only the first, so
/// reordering hands the geometry to whichever child moved out of slot zero -
/// and because `hit_test_node` walks children in reverse, the unmeasured child
/// is tested *first* and would win the click over the child that is actually
/// laid out. `KeyListener` behaves the same way, and `SplitPane` does beyond
/// two children.
///
/// This was never a missed compose descent - composition faithfully composed a
/// stale `size` - which is why it stays a named test next to the property
/// sweep, whose `append` mutation now also grows containers past the arity they
/// document.
#[test]
fn unmeasured_child_does_not_keep_stale_geometry() {
    let viewport = LogicalSize::new(200.0, 160.0);
    let build = |reordered: bool| {
        let mut spec = Spec::new(Kind::Flex {
            axis: Axis::Vertical,
            gap: 0.0,
            padding: 0.0,
            background: None,
        });
        let mut align = Spec::new(Kind::Align {
            alignment: Alignment::TopLeading,
            padding: 0.0,
        });
        let first = Spec::new(Kind::Boxed {
            size: LogicalSize::new(30.0, 20.0),
            color: Color::WHITE,
            semantic: true,
            interactive: true,
        });
        let second = Spec::new(Kind::Boxed {
            size: LogicalSize::new(50.0, 40.0),
            color: Color::BLUE,
            semantic: true,
            interactive: true,
        });
        align.children = if reordered {
            vec![second, first]
        } else {
            vec![first, second]
        };
        spec.children.push(align);
        instantiate(&spec, viewport)
    };

    // The incremental tree lays out the first box, then reorders so that box is
    // no longer the one `Align` measures.
    let (mut live, state) = build(false);
    live.update_passes();
    let align = state.children[0].id();
    let children = state.children[0]
        .children
        .iter()
        .map(Spec::id)
        .collect::<Vec<_>>();
    live.set_children(align, &[children[1], children[0]]);
    live.update_passes();

    let mut reordered = state.clone();
    reordered.children[0].children.swap(0, 1);
    assert_equivalent(
        "Align child moved out of slot zero",
        &mut live,
        &reordered,
        viewport,
    );
}

/// Size a node reports through its own accessible bounds.
///
/// `UiTree` exposes no geometry reader, so the semantic snapshot stands in - and
/// it is the better witness anyway, since reporting bounds that describe nothing
/// is one of the three things stale geometry causes.
fn reported_size(ui: &UiTree, id: NodeId) -> LogicalSize {
    ui.semantic_snapshot()
        .into_iter()
        .find(|node| node.id == id)
        .expect("a semantic node")
        .bounds
        .size
}

/// A measured subtree reparented under an unmeasured one must be zeroed.
///
/// This is the mirror image of the case above, and the sweep's `reparent`
/// mutation is what found it. `zero_layout` used to stop at a node that was
/// already fully zeroed, on the reasoning that nothing could have measured a
/// descendant without measuring that node first. `reparent` breaks it from the
/// other direction: a descendant does not have to be measured *there*, it can
/// arrive already carrying geometry from a parent that did measure it. The
/// grafted subtree then kept painting and hit-testing at its old rectangle,
/// inside a parent whose own geometry was zero.
///
/// `Label` is the host because it measures no children at all, so the graft
/// point is unmeasured for the whole of the node's life there, and a second
/// `Label` sits above it for the same reason - that leaves the graft point in
/// the fully zeroed state the exit fired on.
#[test]
fn a_subtree_reparented_under_an_unmeasured_node_is_zeroed() {
    let viewport = LogicalSize::new(240.0, 200.0);
    let mut spec = Spec::new(Kind::Flex {
        axis: Axis::Vertical,
        gap: 0.0,
        padding: 0.0,
        background: None,
    });
    // The outer `Label` measures no children, so the `Label` beneath it is zeroed on
    // the first pass and stays that way. That `Label` is the graft point, and it
    // has to be zeroed itself rather than merely childless: the exit fired on
    // the graft point's own state, so a measured graft point would have had its
    // new child discarded correctly.
    let mut host = Spec::new(Kind::Text {
        text: "host".to_owned(),
        font_size: 14.0,
        color: Color::WHITE,
        width: None,
    });
    host.children.push(Spec::new(Kind::Text {
        text: "unmeasured".to_owned(),
        font_size: 14.0,
        color: Color::WHITE,
        width: None,
    }));
    let moved = Spec::new(Kind::Boxed {
        size: LogicalSize::new(60.0, 45.0),
        color: Color::BLUE,
        semantic: true,
        interactive: true,
    });
    spec.children.push(host);
    spec.children.push(moved);
    let (mut live, state) = instantiate(&spec, viewport);
    live.update_passes();

    let graft = state.children[0].children[0].id();
    let moved_id = state.children[1].id();
    // The graft point is zeroed and the subtree about to move is not, which is
    // the pairing that made the stale geometry survive.
    assert_eq!(reported_size(&live, graft), LogicalSize::ZERO);
    assert_ne!(reported_size(&live, moved_id), LogicalSize::ZERO);

    live.reparent(moved_id, graft, 0);
    live.update_passes();

    let mut moved_state = state.clone();
    let subtree = moved_state.children.remove(1);
    moved_state.children[0].children[0].children.push(subtree);
    assert_equivalent(
        "subtree reparented under a node that is itself unmeasured",
        &mut live,
        &moved_state,
        viewport,
    );
    assert_eq!(
        reported_size(&live, moved_id),
        LogicalSize::ZERO,
        "an unmeasured graft keeps no geometry"
    );
}
