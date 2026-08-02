//! Entity runtime and declarative UI core for RXUI.

#![warn(missing_docs)]

use std::{
    any::{Any, TypeId, type_name},
    cell::{Cell, Ref, RefCell},
    collections::{BTreeSet, VecDeque},
    fmt,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::{Rc, Weak},
};

use astrelis_core::geometry::LogicalSize;
use astrelis_text::FontDatabase;
use rxui_tree::{Flex, UiTree};

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
    depth: u32,
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
                depth: self.cell.depth,
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

/// A lightweight UI description returned by [`Render`].
///
/// Stage 3B adds the concrete element kinds and fluent builders while
/// preserving this type and the `Render` signature.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element;

/// Stateful entity whose current state can produce a UI description.
pub trait Render: 'static + Sized {
    /// Builds the entity's current lightweight UI description.
    fn render(&mut self, context: &mut Context<'_, Self>) -> Element;
}

/// One-shot UI action addressed to the entity that created it.
type RoutedInvoke = dyn FnOnce(&mut dyn Any, &mut App);

/// One-shot UI action addressed to the entity that created it.
pub struct RoutedHandler {
    target: EntityId,
    invoke: Box<RoutedInvoke>,
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

/// Application-owned entity runtime, retained tree, and deferred work queues.
pub struct App {
    entities: Vec<EntitySlot>,
    free_entities: Vec<u32>,
    tree: UiTree,
    effects: VecDeque<DeferredEffect>,
    subscriptions: Vec<SubscriptionEntry>,
    notified: BTreeSet<(u32, EntityId)>,
    update_depth: usize,
    flushing: bool,
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
            update_depth: 0,
            flushing: false,
        }
    }

    /// Creates a root-level entity.
    pub fn new_entity<T: 'static>(
        &mut self,
        initialize: impl FnOnce(&mut Context<'_, T>) -> T,
    ) -> Entity<T> {
        self.new_entity_at(0, initialize)
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
            depth,
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
            self.flush();
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
    pub fn flush(&mut self) {
        if self.flushing || self.update_depth != 0 {
            return;
        }
        self.flushing = true;
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut processed = 0;
            while let Some(effect) = self.effects.pop_front() {
                processed += 1;
                assert!(
                    processed <= MAX_EFFECTS_PER_FLUSH,
                    "deferred-effect flush exceeded {MAX_EFFECTS_PER_FLUSH} entries; probable emit cycle"
                );
                match effect {
                    DeferredEffect::Notify(id) => {
                        if let Some(cell) = self.resolve(id) {
                            self.notified.insert((cell.depth, id));
                        }
                    }
                    DeferredEffect::Emit {
                        emitter,
                        event_type,
                        event,
                    } => self.deliver_event(emitter, event_type, event.as_ref()),
                }
            }
            self.prune_subscriptions();
            self.prune_dropped_entities();
        }));
        self.flushing = false;
        if let Err(panic) = result {
            resume_unwind(panic);
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
                    self.flush();
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
        let entries = std::mem::take(&mut self.subscriptions);
        let mut retained = Vec::with_capacity(entries.len());
        for entry in entries {
            if !(entry.alive)() || entry.active.upgrade().is_none() {
                continue;
            }
            let matching = entry.emitter == emitter && entry.event_type == event_type;
            let keep = !matching || (entry.invoke)(event, self);
            if keep && (entry.alive)() && entry.active.upgrade().is_some() {
                retained.push(entry);
            }
        }
        retained.append(&mut self.subscriptions);
        self.subscriptions = retained;
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

    /// Creates a one-shot routed handler bound to this entity's weak identity.
    pub fn listener(
        &self,
        listener: impl FnOnce(&mut T, (), &mut Context<'_, T>) + 'static,
    ) -> RoutedHandler {
        let target = self.current;
        let depth = self.depth;
        RoutedHandler {
            target,
            invoke: Box::new(move |state, app| {
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
