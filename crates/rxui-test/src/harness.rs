//! Headless driver for retained RXUI trees.

use astrelis_core::geometry::{LogicalPoint, LogicalRect};
use astrelis_platform::{
    CursorIcon, DeviceId, ElementState, Key, KeyLocation, KeyboardInput, Modifiers, NamedKey,
    PhysicalKey,
};
use rxui_tree::{
    ClipboardOperation, NodeId, PassStats, SemanticAction, SemanticNode, UiInput, UiTree,
};

use crate::SemanticScene;

/// Headless input driver and semantic query surface.
///
/// The harness addresses the tree the way an assistive client does: by
/// accessible label. Tests therefore state *what* they interact with instead of
/// recomputing hit points from semantic bounds, and a layout change that moves a
/// control no longer needs a test edit.
///
/// A harness owns an already assembled [`UiTree`] and settles it immediately.
/// Every driving method also settles any invalidated passes before returning,
/// making its semantic and diagnostic reads describe the completed operation.
pub struct Harness {
    tree: UiTree,
    stats: PassStats,
}

impl Harness {
    /// Takes ownership of a hand-built tree and runs its initial retained passes.
    pub fn new(mut tree: UiTree) -> Self {
        let stats = tree.update_passes().stats;
        Self { tree, stats }
    }

    fn input(&mut self, input: UiInput) {
        self.tree.dispatch(input);
        self.settle();
    }

    fn settle(&mut self) {
        self.stats = if self.tree.needs_update() {
            self.tree.update_passes().stats
        } else {
            PassStats {
                hit_test_nodes: self.tree.stats().hit_test_nodes,
                ..PassStats::default()
            }
        };
    }

    /// Presses and releases the primary pointer button over `label`'s centre.
    pub fn click(&mut self, label: &str) {
        self.click_at(centre(self.bounds(label)));
    }

    /// Presses and releases the primary pointer button at an exact point.
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

    /// Presses one named key with no active modifiers.
    pub fn press(&mut self, key: NamedKey) {
        self.input(UiInput::Keyboard {
            input: key_press(Key::Named(key), None),
            modifiers: Modifiers::default(),
        });
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

    /// Activates `label` through the accessibility layer.
    pub fn activate(&mut self, label: &str) {
        self.semantic_action(label, SemanticAction::Activate);
    }

    /// Applies one platform semantic operation to `label`.
    pub fn semantic_action(&mut self, label: &str, action: SemanticAction) {
        let target = self.find(label).id;
        self.tree.perform_semantic_action(target, action);
        self.settle();
    }

    /// Moves keyboard focus to the first focusable element in the whole tree.
    pub fn focus_first(&mut self) {
        let root = self.tree.root();
        self.tree.focus_first_in_subtree(root);
        self.settle();
    }

    /// Applies pending engine clipboard writes synchronously and returns their count.
    pub fn run_pending_services(&mut self, clipboard: &mut impl Clipboard) -> usize {
        let operations = self.tree.drain_clipboard().collect::<Vec<_>>();
        let count = operations.len();
        for operation in operations {
            match operation {
                ClipboardOperation::WriteText(text) => clipboard.write_text(text),
            }
        }
        count
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
        self.tree.semantic_snapshot()
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
        self.tree.cursor_icon()
    }

    /// Returns the focused retained identity, if any.
    pub fn focused(&self) -> Option<NodeId> {
        self.tree.focused()
    }

    /// Returns engine counters from the most recently completed harness operation.
    pub const fn stats(&self) -> PassStats {
        self.stats
    }

    /// Reads the retained tree through a caller-supplied projection.
    pub fn with_tree<R>(&self, read: impl FnOnce(&UiTree) -> R) -> R {
        read(&self.tree)
    }

    /// Borrows the retained tree for mutations between driven inputs.
    pub const fn tree_mut(&mut self) -> &mut UiTree {
        &mut self.tree
    }
}

/// Clipboard abstraction used by the deterministic harness.
pub trait Clipboard {
    /// Returns clipboard text when available.
    fn read_text(&mut self) -> Option<String>;

    /// Replaces clipboard text.
    fn write_text(&mut self, text: String);
}

/// In-memory clipboard for tests and headless tools.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryClipboard {
    text: Option<String>,
}

impl MemoryClipboard {
    /// Creates a clipboard with optional initial text.
    pub fn new(text: impl Into<Option<String>>) -> Self {
        Self { text: text.into() }
    }

    /// Reads the current value without mutating it.
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }
}

impl Clipboard for MemoryClipboard {
    fn read_text(&mut self) -> Option<String> {
        self.text.clone()
    }

    fn write_text(&mut self, text: String) {
        self.text = Some(text);
    }
}

fn centre(bounds: LogicalRect) -> LogicalPoint {
    LogicalPoint::new(
        bounds.origin.x + bounds.size.width * 0.5,
        bounds.origin.y + bounds.size.height * 0.5,
    )
}

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
