//! Entity runtime and declarative UI core for RXUI.

#![warn(missing_docs)]

use std::{
    any::{Any, TypeId, type_name},
    cell::{Cell, Ref, RefCell},
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::{Rc, Weak},
};

use astrelis_core::geometry::LogicalSize;
use astrelis_text::FontDatabase;
use rxui_tree::{Flex, NodeId, PassStats, UiTree};

pub mod diagnostics;
mod element;
pub mod forms;
mod harness;
mod reconcile;
mod theme;

pub use diagnostics::ViewStats;
pub use element::{
    CustomElementSpec, Element, Key, button, checkbox, column, custom, label, list, row, scroll,
    slider, split_pane, text_field,
};
pub use harness::{EntityHarness, HarnessScope};
pub use rxui_tree::{Axis, ScrollAxis};
pub use theme::Theme;

use element::{EmbeddedEntity, RenderFn};
use reconcile::Mounted;

/// Maximum number of deferred effects one flush may execute.
///
/// The cap turns an accidental self-emitting subscription cycle into a clear
/// authoring error instead of hanging the UI thread. Effects appended by a
/// handler otherwise remain part of the current flush.
const MAX_EFFECTS_PER_FLUSH: usize = 65_536;

/// Stable, generational identity of one entity in an [`App`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId {
    index: u32,
    generation: u32,
}

impl EntityId {
    /// Returns the slot index used by this identity.
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Returns the slot generation used by this identity.
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl fmt::Debug for EntityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "EntityId({}:{})", self.index, self.generation)
    }
}

struct EntityCell {
    id: EntityId,
    depth: Cell<u32>,
    type_id: TypeId,
    type_name: &'static str,
    updating: Cell<bool>,
    value: RefCell<Option<Box<dyn Any>>>,
}

/// Strong, typed handle to state owned by an [`App`].
///
/// Cloning the handle keeps the entity alive. The app intentionally stores a
/// weak reference, so dropping the final strong handle makes future routed
/// work fail its weak upgrade and be pruned.
pub struct Entity<T: 'static> {
    cell: Rc<EntityCell>,
    marker: PhantomData<fn() -> T>,
}

impl<T: 'static> Clone for Entity<T> {
    fn clone(&self) -> Self {
        Self {
            cell: self.cell.clone(),
            marker: PhantomData,
        }
    }
}

impl<T: 'static> fmt::Debug for Entity<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Entity")
            .field("id", &self.id())
            .field("type", &type_name::<T>())
            .finish()
    }
}

impl<T: 'static> Entity<T> {
    /// Returns this entity's stable generational identity.
    pub fn id(&self) -> EntityId {
        self.cell.id
    }

    /// Creates a non-owning handle to this entity.
    pub fn downgrade(&self) -> WeakEntity<T> {
        WeakEntity {
            cell: Rc::downgrade(&self.cell),
            id: self.id(),
            marker: PhantomData,
        }
    }

    /// Borrows the entity's state for reading.
    ///
    /// Panics when `app` is not the app that created this entity, or when the
    /// entity is currently being initialized or updated.
    pub fn read<'a>(&'a self, app: &'a App) -> Ref<'a, T> {
        app.assert_registered(&self.cell);
        assert!(
            !self.cell.updating.get(),
            "cannot read {id:?} while it is being updated",
            id = self.id()
        );
        Ref::map(self.cell.value.borrow(), |value| {
            value
                .as_deref()
                .and_then(<dyn Any>::downcast_ref::<T>)
                .expect("entity state type must match its typed handle")
        })
    }

    /// Mutates the entity and then flushes effects queued by the outer update.
    ///
    /// Updating another entity from `update` is supported. Attempting to update
    /// this same entity again before the callback returns panics with a clear
    /// authoring error.
    pub fn update<R>(
        &self,
        app: &mut App,
        update: impl FnOnce(&mut T, &mut Context<'_, T>) -> R,
    ) -> R {
        app.assert_registered(&self.cell);
        let id = self.id();
        app.update_cell(self.cell.clone(), move |state, app| {
            let state = state
                .downcast_mut::<T>()
                .expect("entity state type must match its typed handle");
            let mut context = Context {
                app,
                current: id,
                depth: self.cell.depth.get(),
                marker: PhantomData,
            };
            update(state, &mut context)
        })
    }
}

/// Non-owning, typed handle to an entity.
pub struct WeakEntity<T: 'static> {
    cell: Weak<EntityCell>,
    id: EntityId,
    marker: PhantomData<fn() -> T>,
}

impl<T: 'static> Clone for WeakEntity<T> {
    fn clone(&self) -> Self {
        Self {
            cell: self.cell.clone(),
            id: self.id,
            marker: PhantomData,
        }
    }
}

impl<T: 'static> fmt::Debug for WeakEntity<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WeakEntity")
            .field("id", &self.id)
            .field("type", &type_name::<T>())
            .finish()
    }
}

impl<T: 'static> WeakEntity<T> {
    /// Returns the identity this weak handle refers to.
    pub const fn id(&self) -> EntityId {
        self.id
    }

    /// Upgrades this handle while the entity still has a strong owner.
    pub fn upgrade(&self) -> Option<Entity<T>> {
        self.cell.upgrade().map(|cell| Entity {
            cell,
            marker: PhantomData,
        })
    }
}

/// Marker implemented by an entity state for every event type it may emit.
pub trait EventEmitter<E: 'static>: 'static {}

/// Stateful entity whose current state can produce a UI description.
pub trait Render: 'static + Sized {
    /// Builds the entity's current lightweight UI description.
    fn render(&mut self, context: &mut Context<'_, Self>) -> Element;
}

/// Reusable UI action addressed to the entity that created it.
type RoutedInvoke = dyn Fn(&mut dyn Any, &mut App);
type RoutedValueInvoke<V> = dyn Fn(V, &mut dyn Any, &mut App);

/// Cloneable, reusable UI action addressed to the entity that created it.
#[derive(Clone)]
pub struct RoutedHandler {
    target: EntityId,
    invoke: Rc<RoutedInvoke>,
}

/// Reusable UI action carrying a proposed controlled value to its owning entity.
pub struct RoutedValueHandler<V: 'static> {
    target: EntityId,
    invoke: Rc<RoutedValueInvoke<V>>,
}

impl<V: 'static> Clone for RoutedValueHandler<V> {
    fn clone(&self) -> Self {
        Self {
            target: self.target,
            invoke: self.invoke.clone(),
        }
    }
}

impl<V: Clone + 'static> RoutedValueHandler<V> {
    /// Binds one value to this routed listener, producing a no-argument action.
    pub fn with(&self, value: V) -> RoutedHandler {
        let target = self.target;
        let invoke = self.invoke.clone();
        RoutedHandler {
            target,
            invoke: Rc::new(move |state, app| invoke(value.clone(), state, app)),
        }
    }

    /// Adapts proposals of another value type before routing them to the owner.
    pub fn map<U: Clone + 'static>(&self, map: impl Fn(U) -> V + 'static) -> RoutedValueHandler<U> {
        let target = self.target;
        let invoke = self.invoke.clone();
        RoutedValueHandler {
            target,
            invoke: Rc::new(move |value, state, app| invoke(map(value), state, app)),
        }
    }
}

impl fmt::Debug for RoutedHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RoutedHandler")
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

impl RoutedHandler {
    /// Returns the target entity identity.
    pub const fn target(&self) -> EntityId {
        self.target
    }
}

struct SubscriptionState;

/// RAII handle that keeps an event subscription active.
///
/// Dropping the handle cancels the subscription. Its erased entry is removed
/// on the next flush or emission.
pub struct Subscription {
    _state: Rc<SubscriptionState>,
}

impl fmt::Debug for Subscription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Subscription")
            .finish_non_exhaustive()
    }
}

impl Subscription {
    /// Explicitly consumes and cancels this subscription.
    pub fn unsubscribe(self) {}

    fn new() -> (Self, Weak<SubscriptionState>) {
        let state = Rc::new(SubscriptionState);
        (
            Self {
                _state: state.clone(),
            },
            Rc::downgrade(&state),
        )
    }
}

type SubscriptionCallback = dyn Fn(&dyn Any, &mut App) -> bool;

struct SubscriptionEntry {
    emitter: EntityId,
    event_type: TypeId,
    active: Weak<SubscriptionState>,
    alive: Box<dyn Fn() -> bool>,
    invoke: Box<SubscriptionCallback>,
}

enum DeferredEffect {
    Notify(EntityId),
    Emit {
        emitter: EntityId,
        event_type: TypeId,
        event: Box<dyn Any>,
    },
}

struct EntitySlot {
    generation: u32,
    cell: Weak<EntityCell>,
    free: bool,
}

struct Renderer {
    cell: Rc<EntityCell>,
    render: RenderFn,
    boundary: NodeId,
    mounted: Option<Mounted>,
}

/// Deterministic work counters returned by one application flush.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlushStats {
    /// Work performed by retained layout, composition, paint, and semantics.
    pub passes: PassStats,
    /// Work performed by entity rendering and description reconciliation.
    pub views: ViewStats,
}

/// Application-owned entity runtime, retained tree, and deferred work queues.
pub struct App {
    entities: Vec<EntitySlot>,
    free_entities: Vec<u32>,
    tree: UiTree,
    effects: VecDeque<DeferredEffect>,
    subscriptions: Vec<SubscriptionEntry>,
    notified: BTreeSet<(u32, EntityId)>,
    renderers: BTreeMap<EntityId, Renderer>,
    update_depth: usize,
    flushing: bool,
    theme: Theme,
}

impl App {
    /// Creates an app and its retained tree for `viewport` and `fonts`.
    pub fn new(viewport: LogicalSize, fonts: FontDatabase) -> Self {
        Self {
            entities: Vec::new(),
            free_entities: Vec::new(),
            tree: UiTree::with_fonts(Flex::default(), viewport, fonts),
            effects: VecDeque::new(),
            subscriptions: Vec::new(),
            notified: BTreeSet::new(),
            renderers: BTreeMap::new(),
            update_depth: 0,
            flushing: false,
            theme: Theme::dark(),
        }
    }

    /// Creates a root-level entity.
    pub fn new_entity<T: 'static>(
        &mut self,
        initialize: impl FnOnce(&mut Context<'_, T>) -> T,
    ) -> Entity<T> {
        self.new_entity_at(0, initialize)
    }

    /// Mounts a renderable entity below the retained root and renders it.
    ///
    /// The application retains the mounted entity until its boundary is
    /// removed. Mounting the same entity more than once is an authoring error.
    pub fn mount<T: Render>(&mut self, entity: &Entity<T>) -> FlushStats {
        self.assert_registered(&entity.cell);
        assert!(
            !self.renderers.contains_key(&entity.id()),
            "entity {:?} is already mounted",
            entity.id()
        );
        let boundary = self
            .tree
            .append(self.tree.root(), rxui_tree::Frame::default());
        self.register_renderer(
            EmbeddedEntity {
                cell: entity.cell.clone(),
                render: render_entity::<T>,
            },
            boundary,
        );
        self.flush()
    }

    fn new_entity_at<T: 'static>(
        &mut self,
        depth: u32,
        initialize: impl FnOnce(&mut Context<'_, T>) -> T,
    ) -> Entity<T> {
        self.prune_dropped_entities();
        let id = self.allocate_id();
        let cell = Rc::new(EntityCell {
            id,
            depth: Cell::new(depth),
            type_id: TypeId::of::<T>(),
            type_name: type_name::<T>(),
            updating: Cell::new(true),
            value: RefCell::new(None),
        });
        self.entities[id.index as usize].cell = Rc::downgrade(&cell);
        let entity = Entity {
            cell: cell.clone(),
            marker: PhantomData,
        };

        self.update_depth += 1;
        let initialized = catch_unwind(AssertUnwindSafe(|| {
            let mut context = Context {
                app: self,
                current: id,
                depth,
                marker: PhantomData,
            };
            initialize(&mut context)
        }));
        self.update_depth -= 1;
        cell.updating.set(false);
        match initialized {
            Ok(value) => *cell.value.borrow_mut() = Some(Box::new(value)),
            Err(panic) => resume_unwind(panic),
        }
        if self.update_depth == 0 && !self.flushing {
            self.flush_effects();
        }
        entity
    }

    /// Executes one routed handler if its weak target is still alive.
    ///
    /// Returns `false` when the target was dropped; stale handlers are ignored.
    pub fn dispatch(&mut self, handler: RoutedHandler) -> bool {
        let Some(cell) = self.resolve(handler.target) else {
            return false;
        };
        self.update_cell(cell, |state, app| (handler.invoke)(state, app));
        true
    }

    /// Drains deferred effects, including effects appended by handlers.
    ///
    /// Calls made while a flush is already active simply return: the outer
    /// drain observes the shared queue. A flush panics after 65,536 effects to
    /// diagnose a cyclic emitter graph instead of hanging indefinitely.
    pub fn flush(&mut self) -> FlushStats {
        self.flush_internal(true)
    }

    /// Returns the current application theme.
    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Replaces the application theme and invalidates every mounted entity
    /// when its revision changes.
    pub fn set_theme(&mut self, theme: Theme) {
        if self.theme == theme {
            return;
        }
        self.theme = theme;
        self.notified.extend(
            self.renderers
                .iter()
                .map(|(id, renderer)| (renderer.cell.depth.get(), *id)),
        );
    }

    /// Routes one retained-tree input payload through its target entity and
    /// settles the resulting entity and retained work.
    pub fn dispatch_input(&mut self, input: rxui_tree::UiInput) -> FlushStats {
        self.route_input(input);
        self.flush()
    }

    /// Routes retained-tree input without running render or retained passes.
    ///
    /// Native hosts use this to coalesce every input derived from one platform
    /// event, inspect [`Self::needs_flush`], and schedule at most one pass.
    pub fn route_input(&mut self, input: rxui_tree::UiInput) {
        let action = self.tree.dispatch(input);
        self.route_erased(action);
    }

    /// Routes one accessibility action through its target entity and settles.
    pub fn perform_semantic_action(
        &mut self,
        target: NodeId,
        action: rxui_tree::SemanticAction,
    ) -> FlushStats {
        self.route_semantic_action(target, action);
        let mut stats = self.flush();
        stats.passes.hit_test_nodes = 0;
        stats
    }

    /// Routes one accessibility action without running render or retained passes.
    ///
    /// This is the semantic counterpart of [`Self::route_input`] for native
    /// accessibility adapters that batch requests with a platform event.
    pub fn route_semantic_action(&mut self, target: NodeId, action: rxui_tree::SemanticAction) {
        let action = self.tree.perform_semantic_action(target, action);
        self.route_erased(action);
    }

    /// Reports whether entity reconciliation or a retained pass is pending.
    ///
    /// Observational only: native scheduling reads this before [`Self::flush`]
    /// clears the queued work.
    pub fn needs_flush(&self) -> bool {
        !self.notified.is_empty() || !self.effects.is_empty() || self.tree.needs_update()
    }

    /// Reports whether at least one mounted entity is queued to render.
    ///
    /// A native host treats this as potentially visible work, while a retained
    /// accessibility-only invalidation can remain pass-only.
    pub fn needs_render(&self) -> bool {
        !self.notified.is_empty()
    }

    fn route_erased(&mut self, action: Option<Box<dyn Any>>) {
        if let Some(action) = action {
            let handler = action
                .downcast::<RoutedHandler>()
                .expect("retained action payload must be a RoutedHandler");
            let _ = self.dispatch(*handler);
        }
    }

    fn flush_effects(&mut self) {
        let _ = self.flush_internal(false);
    }

    fn flush_internal(&mut self, render: bool) -> FlushStats {
        if self.flushing || self.update_depth != 0 {
            return FlushStats::default();
        }
        if render {
            let _ = ViewStats::take();
        }
        self.flushing = true;
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut processed = 0;
            loop {
                while let Some(effect) = self.effects.pop_front() {
                    processed += 1;
                    assert!(
                        processed <= MAX_EFFECTS_PER_FLUSH,
                        "deferred-effect flush exceeded {MAX_EFFECTS_PER_FLUSH} entries; probable emit cycle"
                    );
                    match effect {
                        DeferredEffect::Notify(id) => {
                            if let Some(cell) = self.resolve(id) {
                                self.notified.insert((cell.depth.get(), id));
                            }
                        }
                        DeferredEffect::Emit {
                            emitter,
                            event_type,
                            event,
                        } => self.deliver_event(emitter, event_type, event.as_ref()),
                    }
                }
                if !render {
                    break;
                }
                let Some((_, id)) = self.notified.pop_first() else {
                    break;
                };
                let Some(mut renderer) = self.renderers.remove(&id) else {
                    continue;
                };
                let owner_depth = renderer.cell.depth.get();
                let rendered = catch_unwind(AssertUnwindSafe(|| {
                    ViewStats::record_component_view();
                    let element = (renderer.render)(renderer.cell.clone(), self);
                    reconcile::reconcile_root(
                        &mut renderer.mounted,
                        element,
                        renderer.boundary,
                        owner_depth,
                        self,
                    );
                }));
                self.renderers.insert(id, renderer);
                if let Err(panic) = rendered {
                    resume_unwind(panic);
                }
            }
            self.prune_subscriptions();
            self.prune_dropped_entities();
            if render {
                self.tree.update_passes().stats
            } else {
                PassStats::default()
            }
        }));
        self.flushing = false;
        match result {
            Ok(passes) => FlushStats {
                passes,
                views: render.then(ViewStats::take).unwrap_or_default(),
            },
            Err(panic) => {
                if render {
                    let _ = ViewStats::take();
                }
                resume_unwind(panic)
            }
        }
    }

    /// Returns shared access to the retained tree.
    pub const fn tree(&self) -> &UiTree {
        &self.tree
    }

    /// Returns mutable access to the retained tree.
    pub fn tree_mut(&mut self) -> &mut UiTree {
        &mut self.tree
    }

    /// Reports whether an entity is still registered and strongly owned.
    pub fn contains_entity(&self, id: EntityId) -> bool {
        self.resolve(id).is_some()
    }

    /// Reports whether a notification for `id` has reached the dirty queue.
    ///
    /// Stage 3B consumes this depth-keyed set during reconciliation.
    pub fn is_notified(&self, id: EntityId) -> bool {
        self.notified.iter().any(|(_, queued)| *queued == id)
    }

    /// Returns the number of effects waiting for the current or next flush.
    pub fn pending_effect_count(&self) -> usize {
        self.effects.len()
    }

    /// Returns the number of retained erased subscription entries.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    fn update_cell<R>(
        &mut self,
        cell: Rc<EntityCell>,
        update: impl FnOnce(&mut dyn Any, &mut App) -> R,
    ) -> R {
        if cell.updating.get() {
            panic!(
                "cannot update {id:?} ({ty}) because it is already being updated",
                id = cell.id,
                ty = cell.type_name
            );
        }
        cell.updating.set(true);
        self.update_depth += 1;
        let mut value = cell.value.borrow_mut();
        let result = catch_unwind(AssertUnwindSafe(|| {
            update(
                value
                    .as_deref_mut()
                    .expect("initialized entity must contain state"),
                self,
            )
        }));
        drop(value);
        self.update_depth -= 1;
        cell.updating.set(false);

        match result {
            Ok(value) => {
                if self.update_depth == 0 && !self.flushing {
                    self.flush_effects();
                }
                value
            }
            Err(panic) => resume_unwind(panic),
        }
    }

    fn deliver_event(&mut self, emitter: EntityId, event_type: TypeId, event: &dyn Any) {
        if self.resolve(emitter).is_none() {
            return;
        }
        // Remove only the callback currently being invoked, so it cannot alias
        // the app borrow. Every other entry stays owned by the app throughout;
        // on unwind the current entry is restored before the panic resumes.
        let mut remaining = self.subscriptions.len();
        let mut index = 0;
        while remaining != 0 {
            remaining -= 1;
            let entry = &self.subscriptions[index];
            if !(entry.alive)() || entry.active.upgrade().is_none() {
                self.subscriptions.remove(index);
                continue;
            }
            let matching = entry.emitter == emitter && entry.event_type == event_type;
            if !matching {
                index += 1;
                continue;
            }

            let entry = self.subscriptions.remove(index);
            let invoked = catch_unwind(AssertUnwindSafe(|| (entry.invoke)(event, self)));
            match invoked {
                Ok(keep) => {
                    if keep && (entry.alive)() && entry.active.upgrade().is_some() {
                        self.subscriptions.insert(index, entry);
                        index += 1;
                    }
                }
                Err(panic) => {
                    self.subscriptions.insert(index, entry);
                    resume_unwind(panic);
                }
            }
        }
    }

    fn prune_subscriptions(&mut self) {
        self.subscriptions
            .retain(|entry| entry.active.upgrade().is_some() && (entry.alive)());
    }

    fn allocate_id(&mut self) -> EntityId {
        if let Some(index) = self.free_entities.pop() {
            let slot = &mut self.entities[index as usize];
            debug_assert!(slot.free);
            slot.free = false;
            slot.generation = slot
                .generation
                .checked_add(1)
                .expect("entity generation exhausted");
            EntityId {
                index,
                generation: slot.generation,
            }
        } else {
            let index = u32::try_from(self.entities.len()).expect("entity slot capacity exhausted");
            self.entities.push(EntitySlot {
                generation: 1,
                cell: Weak::new(),
                free: false,
            });
            EntityId {
                index,
                generation: 1,
            }
        }
    }

    fn resolve(&self, id: EntityId) -> Option<Rc<EntityCell>> {
        let slot = self.entities.get(id.index as usize)?;
        if slot.free || slot.generation != id.generation {
            return None;
        }
        let cell = slot.cell.upgrade()?;
        (cell.id == id).then_some(cell)
    }

    fn assert_registered(&self, cell: &Rc<EntityCell>) {
        let Some(registered) = self.resolve(cell.id) else {
            panic!("entity {:?} is not registered in this App", cell.id);
        };
        assert!(
            Rc::ptr_eq(&registered, cell),
            "entity {:?} belongs to a different App",
            cell.id
        );
        assert_eq!(
            cell.type_id, registered.type_id,
            "entity slot type changed without a generation change"
        );
    }

    fn prune_dropped_entities(&mut self) {
        for (index, slot) in self.entities.iter_mut().enumerate() {
            if !slot.free && slot.cell.strong_count() == 0 {
                slot.cell = Weak::new();
                slot.free = true;
                self.free_entities
                    .push(u32::try_from(index).expect("entity index must fit u32"));
            }
        }
    }

    fn register_renderer(
        &mut self,
        entity: EmbeddedEntity,
        boundary: rxui_tree::NodeHandle<rxui_tree::Frame>,
    ) {
        let id = entity.cell.id;
        assert!(
            self.renderers
                .insert(
                    id,
                    Renderer {
                        cell: entity.cell.clone(),
                        render: entity.render,
                        boundary: boundary.id(),
                        mounted: None,
                    },
                )
                .is_none(),
            "entity {id:?} is already mounted"
        );
        self.notified.insert((entity.cell.depth.get(), id));
    }

    fn set_entity_depth(&mut self, cell: &Rc<EntityCell>, depth: u32) {
        let previous = cell.depth.replace(depth);
        if previous != depth && self.notified.remove(&(previous, cell.id)) {
            self.notified.insert((depth, cell.id));
        }
    }

    fn unregister_renderer(&mut self, id: EntityId) {
        self.notified.retain(|(_, queued)| *queued != id);
        if let Some(renderer) = self.renderers.remove(&id)
            && let Some(mounted) = renderer.mounted
        {
            mounted.forget(self, false);
        }
    }
}

fn render_entity<T: Render>(cell: Rc<EntityCell>, app: &mut App) -> Element {
    let id = cell.id;
    let depth = cell.depth.get();
    app.update_cell(cell, move |state, app| {
        let state = state
            .downcast_mut::<T>()
            .expect("entity state type must match its render entry point");
        let mut context = Context {
            app,
            current: id,
            depth,
            marker: PhantomData,
        };
        state.render(&mut context)
    })
}

/// Services available while initializing or updating one entity.
pub struct Context<'a, T: 'static> {
    app: &'a mut App,
    current: EntityId,
    depth: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T: 'static> Context<'_, T> {
    /// Queues this entity for a later render.
    pub fn notify(&mut self) {
        self.app
            .effects
            .push_back(DeferredEffect::Notify(self.current));
    }

    /// Returns the current application theme.
    pub fn theme(&self) -> &Theme {
        &self.app.theme
    }

    /// Queues a typed event for subscribers after the current borrow ends.
    pub fn emit<E: 'static>(&mut self, event: E)
    where
        T: EventEmitter<E>,
    {
        self.app.effects.push_back(DeferredEffect::Emit {
            emitter: self.current,
            event_type: TypeId::of::<E>(),
            event: Box::new(event),
        });
    }

    /// Subscribes the current entity to typed events from `emitter`.
    ///
    /// The callback receives mutable subscriber state, a strong emitter handle
    /// valid for the callback, the event, and subscriber context. Failed weak
    /// upgrades silently remove the erased entry.
    pub fn subscribe<U: EventEmitter<E>, E: 'static>(
        &mut self,
        emitter: &Entity<U>,
        callback: impl Fn(&mut T, Entity<U>, &E, &mut Context<'_, T>) + 'static,
    ) -> Subscription {
        self.app.assert_registered(&emitter.cell);
        let target = self
            .app
            .resolve(self.current)
            .expect("current entity must remain registered");
        let target = WeakEntity::<T> {
            cell: Rc::downgrade(&target),
            id: self.current,
            marker: PhantomData,
        };
        let weak_emitter = emitter.downgrade();
        let alive_target = target.clone();
        let alive_emitter = weak_emitter.clone();
        let (subscription, active) = Subscription::new();
        self.app.subscriptions.push(SubscriptionEntry {
            emitter: emitter.id(),
            event_type: TypeId::of::<E>(),
            active,
            alive: Box::new(move || {
                alive_target.upgrade().is_some() && alive_emitter.upgrade().is_some()
            }),
            invoke: Box::new(move |event, app| {
                let Some(target) = target.upgrade() else {
                    return false;
                };
                let Some(emitter) = weak_emitter.upgrade() else {
                    return false;
                };
                let Some(event) = event.downcast_ref::<E>() else {
                    return true;
                };
                target.update(app, |target, context| {
                    callback(target, emitter, event, context);
                });
                true
            }),
        });
        subscription
    }

    /// Creates a reusable routed handler bound to this entity's weak identity.
    pub fn listener(
        &self,
        listener: impl Fn(&mut T, (), &mut Context<'_, T>) + 'static,
    ) -> RoutedHandler {
        let target = self.current;
        let depth = self.depth;
        RoutedHandler {
            target,
            invoke: Rc::new(move |state, app| {
                let state = state
                    .downcast_mut::<T>()
                    .expect("routed handler target type mismatch");
                let mut context = Context {
                    app,
                    current: target,
                    depth,
                    marker: PhantomData,
                };
                listener(state, (), &mut context);
            }),
        }
    }

    /// Creates a reusable routed handler for a value proposed by a controlled element.
    pub fn listener_value<V: Clone + 'static>(
        &self,
        listener: impl Fn(&mut T, V, &mut Context<'_, T>) + 'static,
    ) -> RoutedValueHandler<V> {
        let target = self.current;
        let depth = self.depth;
        RoutedValueHandler {
            target,
            invoke: Rc::new(move |value, state, app| {
                let state = state
                    .downcast_mut::<T>()
                    .expect("routed handler target type mismatch");
                let mut context = Context {
                    app,
                    current: target,
                    depth,
                    marker: PhantomData,
                };
                listener(state, value, &mut context);
            }),
        }
    }

    /// Creates a child entity one depth below the current entity.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<U: 'static>(
        &mut self,
        initialize: impl FnOnce(&mut Context<'_, U>) -> U,
    ) -> Entity<U> {
        self.app
            .new_entity_at(self.depth.saturating_add(1), initialize)
    }

    /// Updates another entity without exposing the app borrow.
    pub fn update<U: 'static, R>(
        &mut self,
        entity: &Entity<U>,
        update: impl FnOnce(&mut U, &mut Context<'_, U>) -> R,
    ) -> R {
        entity.update(self.app, update)
    }

    /// Returns the current entity identity.
    pub const fn entity_id(&self) -> EntityId {
        self.current
    }

    /// Returns access to app-level services for advanced integrations.
    pub fn app(&mut self) -> &mut App {
        self.app
    }
}

#[cfg(test)]
mod tests;
