//! Entity-aware headless mounting and input driving.

use std::fmt::Write;

use astrelis_core::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use astrelis_text::FontDatabase;
use rxui_tree::{NodeId, SemanticAction, SemanticNode, UiInput};

use crate::{App, Context, Entity, FlushStats, Render};

/// Type-level owner used only while constructing an [`EntityHarness`].
#[doc(hidden)]
pub struct HarnessScope;

/// A headless application plus one strongly owned mounted root entity.
///
/// The constructor deliberately mirrors application initialization:
/// `EntityHarness::new(|cx| cx.new(|_| Root { ... }))`. Every activation is
/// taken from the retained control, routed through [`App`], and flushed before
/// the method returns.
pub struct EntityHarness<T: Render> {
    app: App,
    root: Entity<T>,
    stats: FlushStats,
}

impl<T: Render> EntityHarness<T> {
    /// Creates an app at the default headless viewport and mounts its root.
    pub fn new(create: impl FnOnce(&mut Context<'_, HarnessScope>) -> Entity<T>) -> Self {
        Self::with_viewport(LogicalSize::new(640.0, 480.0), create)
    }

    /// Creates an app at `viewport` and mounts the entity returned by `create`.
    pub fn with_viewport(
        viewport: LogicalSize,
        create: impl FnOnce(&mut Context<'_, HarnessScope>) -> Entity<T>,
    ) -> Self {
        let mut app = App::new(viewport, FontDatabase::empty());
        let mut root = None;
        let scope = app.new_entity(|cx| {
            root = Some(create(cx));
            HarnessScope
        });
        drop(scope);
        let root = root.expect("entity harness constructor must return a root");
        let stats = app.mount(&root);
        Self { app, root, stats }
    }

    /// Presses and releases the primary pointer at the labeled node's centre.
    pub fn click(&mut self, label: &str) {
        let point = centre(self.find(label).bounds);
        let _ = self.app.dispatch_input(UiInput::PointerPressed(point));
        self.stats = self.app.dispatch_input(UiInput::PointerReleased(point));
    }

    /// Activates the labeled node through accessibility routing.
    pub fn activate(&mut self, label: &str) {
        let target = self.find(label).id;
        self.stats = self
            .app
            .perform_semantic_action(target, SemanticAction::Activate);
    }

    /// Performs an accessibility action against the labeled semantic node.
    pub fn semantic_action(&mut self, label: &str, action: SemanticAction) {
        let target = self.find(label).id;
        self.stats = self.app.perform_semantic_action(target, action);
    }

    /// Dispatches one raw tree input and settles resulting entity work.
    pub fn input(&mut self, input: UiInput) {
        self.stats = self.app.dispatch_input(input);
    }

    /// Requests a render of the root without changing its model.
    pub fn refresh(&mut self) {
        let root = self.root.clone();
        root.update(&mut self.app, |_, context| context.notify());
        self.stats = self.app.flush();
    }

    /// Returns the labeled semantic node, panicking with available labels.
    pub fn find(&self, label: &str) -> SemanticNode {
        self.try_find(label).unwrap_or_else(|| {
            let labels = self
                .semantics()
                .iter()
                .filter(|node| !node.data.label.is_empty())
                .map(|node| node.data.label.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            panic!("no semantic node labeled {label:?}; available: [{labels}]")
        })
    }

    /// Returns a labeled semantic node when present.
    pub fn try_find(&self, label: &str) -> Option<SemanticNode> {
        self.semantics()
            .into_iter()
            .find(|node| node.data.label == label)
    }

    /// Returns the current flat semantic snapshot.
    pub fn semantics(&self) -> Vec<SemanticNode> {
        self.app.tree().semantic_snapshot()
    }

    /// Formats labeled semantic roles and labels for examples and diagnostics.
    pub fn snapshot(&self) -> String {
        let mut snapshot = String::new();
        for node in self
            .semantics()
            .into_iter()
            .filter(|node| !node.data.label.is_empty())
        {
            let _ = writeln!(
                snapshot,
                "{:?} label={:?} value={:?}",
                node.data.role, node.data.label, node.data.value
            );
        }
        snapshot
    }

    /// Returns exact counters from the last completed operation.
    pub const fn stats(&self) -> FlushStats {
        self.stats
    }

    /// Returns the strongly owned root entity.
    pub const fn root(&self) -> &Entity<T> {
        &self.root
    }

    /// Returns shared application access for state and retained identity checks.
    pub const fn app(&self) -> &App {
        &self.app
    }

    /// Returns mutable application access for explicit batches.
    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// Returns a retained node id for the labeled semantic node.
    pub fn node_id(&self, label: &str) -> NodeId {
        self.find(label).id
    }
}

fn centre(bounds: LogicalRect) -> LogicalPoint {
    LogicalPoint::new(
        bounds.origin.x + bounds.size.width * 0.5,
        bounds.origin.y + bounds.size.height * 0.5,
    )
}
