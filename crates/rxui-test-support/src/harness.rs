//! Headless driver for component-native RXUI trees.

use astrelis_core::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use astrelis_platform::{
    CursorIcon, DeviceId, ElementState, Key, KeyLocation, KeyboardInput, Modifiers, NamedKey,
    PhysicalKey,
};
use rxui_core::{
    Clipboard, Component, ComponentHost, Theme,
    core::{NodeId, PassStats, SemanticAction, SemanticNode, UiError, UiInput, UiRoot},
};

use crate::SemanticScene;

/// Headless reducer, reconciler, input driver, and semantic query surface.
///
/// The harness addresses the tree the way an assistive client does: by
/// accessible label. Tests therefore state *what* they interact with instead of
/// recomputing hit points from semantic bounds, and a layout change that moves a
/// control no longer needs a test edit.
///
/// Only [`Harness::new`] returns a [`Result`], because mounting is the one step
/// a test legitimately asserts about (a duplicate view key, for instance). Every
/// driving method panics on a [`UiError`]: mid-test engine failure is a test
/// failure either way, and threading `unwrap()` through every line is exactly
/// the noise this crate exists to remove.
///
/// The surface is deliberately closed: there is no `ComponentHost` escape hatch,
/// so a test that needs a new capability grows this type instead of reintroducing
/// the hand-rolled input plumbing.
pub struct Harness<C: Component> {
    host: ComponentHost<C>,
    stats: PassStats,
}

impl<C: Component> Harness<C> {
    /// Mounts a component in a deterministic logical viewport under the dark theme.
    pub fn new(component: C, viewport: LogicalSize) -> Result<Self, UiError> {
        Ok(Self {
            host: ComponentHost::new(component, viewport, Theme::dark())?,
            stats: PassStats::default(),
        })
    }

    /// Applies one typed component action and reconciles.
    pub fn dispatch(&mut self, action: C::Action) {
        self.stats = self
            .host
            .dispatch(action)
            .expect("dispatching a typed action reconciles")
            .stats;
    }

    /// Rebuilds the entire view tree from unchanged component state.
    ///
    /// This is the only way to observe what a view costs when nothing about it
    /// changed: it marks the root stale and disables props-equality pruning, so
    /// every mounted view is re-diffed against a freshly constructed one. A
    /// well-behaved tree therefore settles with empty [`PassStats`].
    pub fn refresh(&mut self) {
        self.stats = self
            .host
            .refresh()
            .expect("refreshing rebuilds the whole tree")
            .stats;
    }

    /// Mutates application-owned state and reconciles the resulting frame.
    ///
    /// Component state a test edits directly is invisible to the reducer, which
    /// is why this forces the same whole-tree rebuild [`Self::refresh`] does.
    pub fn mutate(&mut self, edit: impl FnOnce(&mut C)) {
        edit(self.host.component_mut());
        self.refresh();
    }

    fn input(&mut self, input: UiInput) {
        self.stats = self
            .host
            .input(input)
            .expect("routing an input reconciles")
            .map(|update| update.stats)
            .unwrap_or_default();
    }

    /// Presses and releases the primary pointer button over `label`'s centre.
    pub fn click(&mut self, label: &str) {
        self.click_at(centre(self.bounds(label)));
    }

    /// Presses and releases the primary pointer button at an exact point.
    ///
    /// Use this when the position within a control is what is under test, such
    /// as caret placement inside a text field or grabbing a splitter that owns
    /// no accessible node of its own.
    pub fn click_at(&mut self, point: LogicalPoint) {
        self.input(UiInput::PointerPressed(point));
        self.input(UiInput::PointerReleased(point));
    }

    /// Presses the primary pointer button at an exact point without releasing.
    pub fn press_pointer_at(&mut self, point: LogicalPoint) {
        self.input(UiInput::PointerPressed(point));
    }

    /// Releases the primary pointer button at an exact point.
    pub fn release_pointer_at(&mut self, point: LogicalPoint) {
        self.input(UiInput::PointerReleased(point));
    }

    /// Moves the pointer over `label`'s centre.
    pub fn hover(&mut self, label: &str) {
        self.hover_at(centre(self.bounds(label)));
    }

    /// Moves the pointer to an exact point.
    pub fn hover_at(&mut self, point: LogicalPoint) {
        self.input(UiInput::PointerMoved(point));
    }

    /// Reports the pointer leaving the native window.
    pub fn pointer_left(&mut self) {
        self.input(UiInput::PointerLeft);
    }

    /// Types `text` one character at a time into the focused element.
    pub fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            let character = character.to_string();
            self.input(UiInput::Keyboard {
                input: key_press(Key::Character(character.clone()), Some(character)),
                modifiers: Modifiers::default(),
            });
        }
    }

    /// Presses one named key with no active modifiers.
    pub fn press(&mut self, key: NamedKey) {
        self.input(UiInput::Keyboard {
            input: key_press(Key::Named(key), None),
            modifiers: Modifiers::default(),
        });
    }

    /// Activates `label` through the accessibility layer.
    pub fn activate(&mut self, label: &str) {
        self.semantic_action(label, SemanticAction::Activate);
    }

    /// Applies one platform semantic operation to `label`.
    pub fn semantic_action(&mut self, label: &str, action: SemanticAction) {
        let target = self.find(label).id;
        self.stats = self
            .host
            .semantic_action(target, action)
            .expect("a semantic action routes through the component reducer")
            .stats;
    }

    /// Moves keyboard focus to the first focusable element in the whole tree.
    pub fn focus_first(&mut self) {
        let root = self.host.ui().root();
        self.host
            .ui_mut()
            .focus_first_in_subtree(root)
            .expect("focusing the root subtree");
        self.stats = self
            .host
            .ui_mut()
            .update_passes()
            .expect("settling after a focus change")
            .stats;
    }

    /// Drains the effects the component escalated to its parent or application.
    pub fn drain_effects(&mut self) -> impl Iterator<Item = C::Effect> + '_ {
        self.host.drain_effects()
    }

    /// Executes every pending host service synchronously, returning the count.
    pub fn run_pending_services(&mut self, clipboard: &mut impl Clipboard) -> usize {
        self.host
            .run_pending_services(clipboard)
            .expect("pending services complete")
    }

    /// Returns the labeled semantic node, panicking with the available labels.
    pub fn find(&self, label: &str) -> SemanticNode {
        self.try_find(label).unwrap_or_else(|| {
            let available = self
                .semantics()
                .iter()
                .filter(|node| !node.data.label.is_empty())
                .map(|node| format!("{:?}", node.data.label))
                .collect::<Vec<_>>()
                .join(", ");
            panic!("no semantic node labeled {label:?}; available labels: [{available}]")
        })
    }

    /// Returns the labeled semantic node if the tree currently publishes one.
    pub fn try_find(&self, label: &str) -> Option<SemanticNode> {
        self.semantics()
            .into_iter()
            .find(|node| node.data.label == label)
    }

    /// Returns `label`'s resolved window-space bounds.
    pub fn bounds(&self, label: &str) -> LogicalRect {
        self.find(label).bounds
    }

    /// Returns the deterministic flat semantic snapshot.
    pub fn semantics(&self) -> Vec<SemanticNode> {
        self.host.ui().semantic_snapshot()
    }

    /// Returns the semantic snapshot normalized to labeled landmarks.
    pub fn scene(&self) -> SemanticScene {
        SemanticScene::from_nodes(&self.semantics())
    }

    /// Formats the current scene as a reviewable text golden.
    pub fn snapshot(&self) -> String {
        self.scene().snapshot()
    }

    /// Returns the cursor the engine currently requests.
    pub fn cursor_icon(&self) -> CursorIcon {
        self.host.ui().cursor_icon()
    }

    /// Returns the focused retained identity, if any.
    pub fn focused(&self) -> Option<NodeId> {
        self.host.ui().focused()
    }

    /// Returns the engine pass counters from the most recent frame update.
    ///
    /// An input the engine discards produces no frame update, and reads back as
    /// [`PassStats::default`] rather than as the previous frame's counters, so a
    /// budget assertion can never pass on stale numbers.
    pub fn stats(&self) -> PassStats {
        self.stats
    }

    /// Reads component state.
    pub const fn component(&self) -> &C {
        self.host.component()
    }

    /// Reads the mounted retained tree through a caller-supplied projection.
    ///
    /// Part of RXUI's public surface takes `&UiRoot` directly - the retained
    /// inspection snapshot, for one - and such an API cannot be exercised
    /// through label lookups. The borrow is immutable and scoped to the
    /// closure, so this stays a read of the mounted tree rather than the
    /// mutable host escape hatch this type exists to avoid.
    pub fn with_ui<R>(&self, read: impl FnOnce(&UiRoot) -> R) -> R {
        read(self.host.ui())
    }
}

fn centre(bounds: LogicalRect) -> LogicalPoint {
    LogicalPoint::new(
        bounds.origin.x + bounds.size.width * 0.5,
        bounds.origin.y + bounds.size.height * 0.5,
    )
}

/// Synthesizes a key press.
///
/// The physical key is left unidentified: RXUI routes on the logical key, and
/// inventing a scancode would imply a keyboard layout the test never chose.
fn key_press(logical_key: Key, text: Option<String>) -> KeyboardInput {
    KeyboardInput {
        device_id: DeviceId(1),
        physical_key: PhysicalKey::Unidentified,
        logical_key,
        text,
        location: KeyLocation::Standard,
        state: ElementState::Pressed,
        repeat: false,
        synthetic: false,
    }
}
