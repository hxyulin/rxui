//! Keyed reconciliation of one container's child list.

use std::{collections::HashMap, ops::ControlFlow};

use astrelis_ui_next::{NodeId, UiError};

use crate::{
    diagnostics::ViewStats,
    view::{AnyView, Mounted, ViewContext, ViewKey},
};
/// Strategy chosen for one child-list reconciliation pass.
enum ChildStrategy {
    /// Pair new views with retained ones by position.
    ///
    /// Used for unkeyed lists, and for keyed lists whose length and key order
    /// are both unchanged - which is the overwhelmingly common case, and the one
    /// that used to build and throw away a `HashMap` per container per frame.
    Positional,
    /// Match retained children by key through the reusable index.
    Remap,
}

/// One container's reconciled child list plus the scratch space it reuses.
///
/// This is the whole of keyed reconciliation, and a custom container gets it by
/// owning one of these: [`build`](Self::build) at mount,
/// [`reconcile`](Self::reconcile) on every pass, and
/// [`visit`](Self::visit) from [`crate::MountedState::visit_children`]. Nothing else is
/// required for a third-party container to preserve retained identity across
/// reorders, host nested components, and publish a minimal child list.
///
/// Everything here exists to keep a steady-state frame allocation-free: the
/// mounted list is edited in place rather than rebuilt, the key index and the
/// remap buffer are cleared instead of dropped, and the published child order is
/// remembered so an unchanged order is never handed to the engine again.
pub struct MountedChildren<Action: 'static> {
    mounted: Vec<Mounted<Action>>,
    /// Child order most recently published to the engine.
    published: Vec<NodeId>,
    /// Previous children held during a keyed remap.
    scratch: Vec<Option<Mounted<Action>>>,
    /// Key index reused by validation and by the keyed remap.
    index: HashMap<ViewKey, usize>,
}

impl<Action: 'static> Default for MountedChildren<Action> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Action: 'static> MountedChildren<Action> {
    /// Creates an empty child list.
    pub fn new() -> Self {
        Self {
            mounted: Vec::new(),
            published: Vec::new(),
            scratch: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Whether no children are mounted.
    pub fn is_empty(&self) -> bool {
        self.mounted.is_empty()
    }

    /// Number of mounted children.
    pub fn len(&self) -> usize {
        self.mounted.len()
    }

    /// Iterates the mounted children in paint order.
    pub fn mounted_mut(&mut self) -> impl Iterator<Item = &mut Mounted<Action>> {
        self.mounted.iter_mut()
    }

    /// Visits each mounted child in paint order, stopping on `Break`.
    ///
    /// Forward [`crate::MountedState::visit_children`] straight to this.
    pub fn visit(&mut self, visit: &mut dyn FnMut(&mut Mounted<Action>) -> ControlFlow<()>) {
        for child in &mut self.mounted {
            if visit(child).is_break() {
                break;
            }
        }
    }

    /// Mounts an initial child list below the context's parent.
    ///
    /// The engine appends children in order, so the published order is recorded
    /// rather than set: there is nothing to reorder yet.
    pub fn build(
        &mut self,
        views: Vec<AnyView<Action>>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        self.validate(&views)?;
        self.mounted.reserve(views.len());
        self.published.reserve(views.len());
        for view in views {
            let built = context.build_child(view)?;
            self.published.push(built.node());
            self.mounted.push(built);
        }
        Ok(())
    }

    /// Reconciles a replacement child list into retained state.
    ///
    /// Errors when the sequence is partially keyed or carries a duplicate key,
    /// which are authoring mistakes rather than recoverable conditions: both
    /// silently lose retained identity on the next insertion.
    pub fn reconcile(
        &mut self,
        views: Vec<AnyView<Action>>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        // `containers_reconciled`: one child-list reconciliation pass. Counted
        // once for both strategies, so every container is covered exactly once.
        ViewStats::record_container_reconciled();
        match self.validate(&views)? {
            ChildStrategy::Positional => self.reconcile_positional(views, context)?,
            ChildStrategy::Remap => self.reconcile_keyed(views, context)?,
        }
        self.publish(context)
    }

    /// Checks keying and picks a strategy in a single scan.
    ///
    /// One pass rejects partially keyed sequences, rejects duplicate keys, and
    /// decides whether the new keys already line up with the retained ones. The
    /// duplicate check reuses `index` instead of allocating a `HashSet` per
    /// container per frame.
    fn validate(&mut self, views: &[AnyView<Action>]) -> Result<ChildStrategy, UiError> {
        let keyed = views.first().is_some_and(|view| view.key.is_some());
        if !keyed {
            if views.iter().any(|view| view.key.is_some()) {
                return Err(UiError::new(
                    "dynamic view sequences must key every child or no children",
                ));
            }
            // An unkeyed list is always paired by position, including when its
            // length changed: trailing children are appended or removed.
            return Ok(ChildStrategy::Positional);
        }
        self.index.clear();
        self.index.reserve(views.len());
        let mut aligned = self.mounted.len() == views.len();
        for (position, view) in views.iter().enumerate() {
            let Some(key) = view.key.as_ref() else {
                return Err(UiError::new(
                    "dynamic view sequences must key every child or no children",
                ));
            };
            if self.index.insert(key.clone(), position).is_some() {
                return Err(UiError::new(format!("duplicate view key `{key}`")));
            }
            if aligned && self.mounted[position].key() != Some(key) {
                aligned = false;
            }
        }
        Ok(if aligned {
            ChildStrategy::Positional
        } else {
            ChildStrategy::Remap
        })
    }

    fn reconcile_positional(
        &mut self,
        views: Vec<AnyView<Action>>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let kept = views.len();
        for (position, view) in views.into_iter().enumerate() {
            if let Some(retained) = self.mounted.get_mut(position) {
                context.rebuild_child(retained, view)?;
            } else {
                let built = context.build_child(view)?;
                self.mounted.push(built);
            }
        }
        while self.mounted.len() > kept {
            let extra = self.mounted.pop().expect("length was checked");
            context.ui().remove(extra.node())?;
        }
        Ok(())
    }

    fn reconcile_keyed(
        &mut self,
        views: Vec<AnyView<Action>>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        // Re-purpose the index built by `validate` as retained key -> slot.
        self.index.clear();
        self.scratch.clear();
        for retained in self.mounted.drain(..) {
            let key = retained.key.clone().expect("validated keyed sequence");
            self.index.insert(key, self.scratch.len());
            self.scratch.push(Some(retained));
        }
        for view in views {
            let key = view.key.as_ref().expect("validated keyed sequence");
            let retained = self
                .index
                .get(key)
                .copied()
                .and_then(|slot| self.scratch[slot].take());
            match retained {
                Some(mut retained) => {
                    context.rebuild_child(&mut retained, view)?;
                    self.mounted.push(retained);
                }
                None => {
                    let built = context.build_child(view)?;
                    self.mounted.push(built);
                }
            }
        }
        for slot in 0..self.scratch.len() {
            if let Some(extra) = self.scratch[slot].take() {
                context.ui().remove(extra.node())?;
            }
        }
        self.scratch.clear();
        Ok(())
    }

    /// Publishes the child order, but only when it actually moved.
    ///
    /// `UiRoot::set_children` invalidates the parent with `Invalidation::ALL`,
    /// so republishing an unchanged order costs a full pass over a container
    /// that did not change at all. Comparing against the last published order
    /// makes that cost proportional to real structural change.
    fn publish(&mut self, context: &mut ViewContext<'_, Action>) -> Result<(), UiError> {
        if self.published.len() == self.mounted.len()
            && self
                .published
                .iter()
                .zip(&self.mounted)
                .all(|(node, view)| *node == view.node())
        {
            return Ok(());
        }
        self.published.clear();
        self.published
            .extend(self.mounted.iter().map(|view| view.node()));
        // `set_children_calls`: the only place the framework hands the engine a
        // whole child list.
        ViewStats::record_set_children();
        let parent = context.parent();
        context.ui().set_children(parent, &self.published)
    }
}
