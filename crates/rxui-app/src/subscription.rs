//! Declarative descriptions of application-scoped long-lived event sources.

use std::{
    any::Any, collections::HashSet, fmt, marker::PhantomData, rc::Rc, sync::Arc, time::Duration,
};

use crate::{Error, Result};

/// Stable identity for one application-scoped subscription.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SubscriptionId {
    namespace: &'static str,
    instance: u64,
}

impl SubscriptionId {
    /// Creates an identity in a static namespace for one feature instance.
    pub const fn new(namespace: &'static str, instance: u64) -> Self {
        Self {
            namespace,
            instance,
        }
    }

    /// Creates an application-wide singleton identity.
    pub const fn singleton(namespace: &'static str) -> Self {
        Self::new(namespace, 0)
    }

    /// Returns the descriptive static namespace.
    pub const fn namespace(self) -> &'static str {
        self.namespace
    }

    /// Returns the stable feature-instance component.
    pub const fn instance(self) -> u64 {
        self.instance
    }
}

impl fmt::Display for SubscriptionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}#{}", self.namespace, self.instance)
    }
}

/// Queue behavior for events produced by a subscription.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeliveryPolicy {
    /// Retain at most the latest pending event for this subscription identity.
    #[default]
    Latest,
    /// Deliver every event produced by the source.
    Every,
}

/// Broad category of one subscription source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubscriptionKind {
    /// A repeating application-clock interval.
    Interval,
    /// A debounced filesystem watcher.
    FileWatch,
}

/// Current lifecycle state of a reconciled subscription.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionStatus {
    /// The underlying event source started successfully.
    Running,
    /// The source failed to start and remains dormant until its configuration changes.
    Failed,
}

/// Read-only metadata for one currently reconciled subscription.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveSubscriptionSnapshot {
    id: SubscriptionId,
    kind: SubscriptionKind,
    delivery: DeliveryPolicy,
    interval: Option<Duration>,
    elapsed: Duration,
    starts: u64,
    status: SubscriptionStatus,
}

impl ActiveSubscriptionSnapshot {
    /// Creates subscription metadata for an alternative backend.
    #[doc(hidden)]
    pub const fn new(
        id: SubscriptionId,
        kind: SubscriptionKind,
        delivery: DeliveryPolicy,
        interval: Option<Duration>,
        elapsed: Duration,
        starts: u64,
        status: SubscriptionStatus,
    ) -> Self {
        Self {
            id,
            kind,
            delivery,
            interval,
            elapsed,
            starts,
            status,
        }
    }

    /// Returns the source's stable identity.
    pub const fn id(&self) -> SubscriptionId {
        self.id
    }
    /// Returns the source category.
    pub const fn kind(&self) -> SubscriptionKind {
        self.kind
    }
    /// Returns the pending-event delivery policy.
    pub const fn delivery_policy(&self) -> DeliveryPolicy {
        self.delivery
    }
    /// Returns the repeating period for interval sources.
    pub const fn interval(&self) -> Option<Duration> {
        self.interval
    }
    /// Returns time since the current source generation started.
    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }
    /// Returns the cumulative number of starts for this identity.
    pub const fn starts(&self) -> u64 {
        self.starts
    }
    /// Returns whether the underlying source is running or failed to start.
    pub const fn status(&self) -> SubscriptionStatus {
        self.status
    }
}

trait ConfigValue: fmt::Debug {
    fn equals(&self, other: &dyn ConfigValue) -> bool;
    fn as_any(&self) -> &dyn Any;
}

impl<T> ConfigValue for T
where
    T: Any + fmt::Debug + Eq,
{
    fn equals(&self, other: &dyn ConfigValue) -> bool {
        other.as_any().downcast_ref::<T>() == Some(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Structural configuration used to decide whether a source restarts.
#[doc(hidden)]
pub struct SubscriptionConfig {
    kind: SubscriptionKind,
    interval: Option<Duration>,
    delivery: DeliveryPolicy,
    source: Option<Box<dyn ConfigValue>>,
}

impl SubscriptionConfig {
    /// Returns the source category.
    #[doc(hidden)]
    pub const fn kind(&self) -> SubscriptionKind {
        self.kind
    }
    /// Returns the interval duration for interval sources.
    #[doc(hidden)]
    pub const fn interval(&self) -> Option<Duration> {
        self.interval
    }
    /// Returns the delivery policy.
    #[doc(hidden)]
    pub const fn delivery(&self) -> DeliveryPolicy {
        self.delivery
    }
}

impl PartialEq for SubscriptionConfig {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.interval == other.interval
            && self.delivery == other.delivery
            && match (&self.source, &other.source) {
                (None, None) => true,
                (Some(left), Some(right)) => left.equals(right.as_ref()),
                _ => false,
            }
    }
}

impl Eq for SubscriptionConfig {}

impl fmt::Debug for SubscriptionConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SubscriptionConfig")
            .field("kind", &self.kind)
            .field("interval", &self.interval)
            .field("delivery", &self.delivery)
            .finish_non_exhaustive()
    }
}

/// Type-erased event payload crossing from a service thread to the UI thread.
#[doc(hidden)]
pub type RawSubscriptionEvent = Box<dyn Any + Send>;

/// Type-erased latest-value event sink used by service integrations.
#[doc(hidden)]
#[derive(Clone)]
pub struct RawSubscriptionSink {
    emit: Arc<dyn Fn(RawSubscriptionEvent) + Send + Sync>,
}

impl RawSubscriptionSink {
    /// Creates a sink from a runtime-owned scheduling callback.
    #[doc(hidden)]
    pub fn new(emit: impl Fn(RawSubscriptionEvent) + Send + Sync + 'static) -> Self {
        Self {
            emit: Arc::new(emit),
        }
    }

    /// Queues one already-erased service event.
    #[doc(hidden)]
    pub fn emit_raw(&self, event: RawSubscriptionEvent) {
        (self.emit)(event);
    }

    fn typed<E: Send + 'static>(self) -> SubscriptionEventSink<E> {
        SubscriptionEventSink {
            raw: self,
            marker: PhantomData,
        }
    }
}

/// Thread-safe event sink passed to one service subscription backend.
#[doc(hidden)]
pub struct SubscriptionEventSink<E> {
    raw: RawSubscriptionSink,
    marker: PhantomData<fn(E)>,
}

impl<E> Clone for SubscriptionEventSink<E> {
    fn clone(&self) -> Self {
        Self {
            raw: self.raw.clone(),
            marker: PhantomData,
        }
    }
}

impl<E: Send + 'static> SubscriptionEventSink<E> {
    /// Queues the latest event for conversion on the UI thread.
    pub fn emit(&self, event: E) {
        (self.raw.emit)(Box::new(event));
    }
}

/// Result of starting one service-owned subscription source.
#[doc(hidden)]
pub struct ServiceSubscriptionStart {
    guard: Option<Box<dyn Any + Send>>,
    failure: Option<RawSubscriptionEvent>,
}

impl ServiceSubscriptionStart {
    /// Returns the live guard and optional startup failure payload.
    #[doc(hidden)]
    pub fn into_parts(self) -> (Option<Box<dyn Any + Send>>, Option<RawSubscriptionEvent>) {
        (self.guard, self.failure)
    }
}

/// Type-erased service subscription recipe used by application backends.
#[doc(hidden)]
pub struct ServiceSubscriptionFactory<M> {
    start: Box<dyn FnOnce(RawSubscriptionSink) -> ServiceSubscriptionStart>,
    decode: Box<dyn FnMut(RawSubscriptionEvent) -> M>,
}

impl<M> ServiceSubscriptionFactory<M> {
    /// Starts the service source with a runtime-owned sink.
    #[doc(hidden)]
    pub fn start(
        self,
        sink: RawSubscriptionSink,
    ) -> (
        ServiceSubscriptionStart,
        Box<dyn FnMut(RawSubscriptionEvent) -> M>,
    ) {
        ((self.start)(sink), self.decode)
    }

    /// Discards the unused starter and returns the newest UI-thread decoder.
    #[doc(hidden)]
    pub fn into_decoder(self) -> Box<dyn FnMut(RawSubscriptionEvent) -> M> {
        self.decode
    }
}

/// Executable portion of a subscription description.
#[doc(hidden)]
pub enum SubscriptionFactory<M> {
    /// A UI-thread interval message factory.
    Interval(Box<dyn FnMut() -> M>),
    /// A service-owned external source and UI-thread decoder.
    Service(ServiceSubscriptionFactory<M>),
}

/// One desired application-scoped event source.
pub struct Subscription<M: 'static> {
    id: SubscriptionId,
    config: SubscriptionConfig,
    factory: SubscriptionFactory<M>,
}

impl<M: 'static> Subscription<M> {
    /// Creates a repeating interval from one cloneable message.
    pub fn interval(id: SubscriptionId, interval: Duration, message: M) -> Self
    where
        M: Clone,
    {
        Self::interval_with(id, interval, move || message.clone())
    }

    /// Creates a repeating interval whose factory runs on the UI thread.
    pub fn interval_with(
        id: SubscriptionId,
        interval: Duration,
        factory: impl FnMut() -> M + 'static,
    ) -> Self {
        assert!(
            !interval.is_zero(),
            "subscription interval must be non-zero"
        );
        Self {
            id,
            config: SubscriptionConfig {
                kind: SubscriptionKind::Interval,
                interval: Some(interval),
                delivery: DeliveryPolicy::Latest,
                source: None,
            },
            factory: SubscriptionFactory::Interval(Box::new(factory)),
        }
    }

    /// Creates a service-owned subscription without exposing a general source trait.
    #[doc(hidden)]
    pub fn service<E, C, G>(
        id: SubscriptionId,
        kind: SubscriptionKind,
        config: C,
        start: impl FnOnce(SubscriptionEventSink<E>) -> std::result::Result<G, E> + 'static,
        mut decode: impl FnMut(E) -> M + 'static,
    ) -> Self
    where
        E: Send + 'static,
        C: Any + fmt::Debug + Eq + 'static,
        G: Any + Send,
    {
        let start = Box::new(move |sink: RawSubscriptionSink| match start(sink.typed()) {
            Ok(guard) => ServiceSubscriptionStart {
                guard: Some(Box::new(guard)),
                failure: None,
            },
            Err(error) => ServiceSubscriptionStart {
                guard: None,
                failure: Some(Box::new(error)),
            },
        });
        let decode = Box::new(move |event: RawSubscriptionEvent| {
            let event = *event
                .downcast::<E>()
                .expect("service subscription emitted its declared event type");
            decode(event)
        });
        Self {
            id,
            config: SubscriptionConfig {
                kind,
                interval: None,
                delivery: DeliveryPolicy::Latest,
                source: Some(Box::new(config)),
            },
            factory: SubscriptionFactory::Service(ServiceSubscriptionFactory { start, decode }),
        }
    }

    /// Changes how pending events from this source are delivered.
    #[must_use]
    pub fn delivery(mut self, delivery: DeliveryPolicy) -> Self {
        self.config.delivery = delivery;
        self
    }

    /// Returns this source's stable identity.
    pub const fn id(&self) -> SubscriptionId {
        self.id
    }
    /// Returns this source's delivery policy.
    pub const fn delivery_policy(&self) -> DeliveryPolicy {
        self.config.delivery
    }
    /// Returns the source category.
    pub const fn kind(&self) -> SubscriptionKind {
        self.config.kind
    }
    /// Returns the repeating period for interval sources.
    pub const fn interval_duration(&self) -> Option<Duration> {
        self.config.interval
    }

    /// Decomposes a description for an alternative application backend.
    #[doc(hidden)]
    pub fn into_parts(self) -> (SubscriptionId, SubscriptionConfig, SubscriptionFactory<M>) {
        (self.id, self.config, self.factory)
    }
}

impl<M: 'static> fmt::Debug for Subscription<M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Subscription")
            .field("id", &self.id)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// Desired application subscriptions returned by [`crate::App::subscriptions`].
pub struct Subscriptions<M: 'static> {
    entries: Vec<Subscription<M>>,
}

impl<M: 'static> Subscriptions<M> {
    /// Creates an empty desired set.
    pub const fn none() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
    /// Creates a desired set containing one source.
    pub fn one(subscription: Subscription<M>) -> Self {
        Self {
            entries: vec![subscription],
        }
    }
    /// Collects a desired set from subscription descriptions.
    pub fn batch(subscriptions: impl IntoIterator<Item = Subscription<M>>) -> Self {
        Self {
            entries: subscriptions.into_iter().collect(),
        }
    }
    /// Appends one desired source.
    pub fn push(&mut self, subscription: Subscription<M>) {
        self.entries.push(subscription);
    }
    /// Appends another desired set.
    pub fn extend(&mut self, subscriptions: Self) {
        self.entries.extend(subscriptions.entries);
    }

    /// Maps feature-local subscription messages into a parent message type.
    pub fn map<Root: 'static>(self, map: impl Fn(M) -> Root + 'static) -> Subscriptions<Root> {
        let map: Rc<dyn Fn(M) -> Root> = Rc::new(map);
        Subscriptions {
            entries: self
                .entries
                .into_iter()
                .map(|subscription| {
                    let map = Rc::clone(&map);
                    let factory = match subscription.factory {
                        SubscriptionFactory::Interval(mut factory) => {
                            SubscriptionFactory::Interval(Box::new(move || map(factory())))
                        }
                        SubscriptionFactory::Service(factory) => {
                            let ServiceSubscriptionFactory { start, mut decode } = factory;
                            SubscriptionFactory::Service(ServiceSubscriptionFactory {
                                start,
                                decode: Box::new(move |event| map(decode(event))),
                            })
                        }
                    };
                    Subscription {
                        id: subscription.id,
                        config: subscription.config,
                        factory,
                    }
                })
                .collect(),
        }
    }

    /// Validates and decomposes a complete desired set for a backend.
    #[doc(hidden)]
    pub fn into_unique(self) -> Result<Vec<Subscription<M>>> {
        let mut ids = HashSet::with_capacity(self.entries.len());
        for subscription in &self.entries {
            if subscription.config.kind == SubscriptionKind::FileWatch
                && subscription.config.delivery != DeliveryPolicy::Latest
            {
                return Err(Error::msg(format!(
                    "filesystem subscription {} supports latest-value delivery only",
                    subscription.id
                )));
            }
            if !ids.insert(subscription.id) {
                return Err(Error::msg(format!(
                    "duplicate subscription id {} ({:?})",
                    subscription.id, subscription.config.kind
                )));
            }
        }
        Ok(self.entries)
    }
}

impl<M: 'static> Default for Subscriptions<M> {
    fn default() -> Self {
        Self::none()
    }
}
impl<M: 'static> From<Subscription<M>> for Subscriptions<M> {
    fn from(subscription: Subscription<M>) -> Self {
        Self::one(subscription)
    }
}
impl<M: 'static> FromIterator<Subscription<M>> for Subscriptions<M> {
    fn from_iter<T: IntoIterator<Item = Subscription<M>>>(iter: T) -> Self {
        Self::batch(iter)
    }
}
impl<M: 'static> fmt::Debug for Subscriptions<M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.entries.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_preserves_identity_and_configuration() {
        let subscriptions = Subscriptions::one(Subscription::interval(
            SubscriptionId::new("editor.poll", 7),
            Duration::from_millis(40),
            3_u32,
        ))
        .map(|value| value.to_string());
        let mut entries = subscriptions.into_unique().unwrap();
        let entry = entries.pop().unwrap();
        assert_eq!(entry.id(), SubscriptionId::new("editor.poll", 7));
        assert_eq!(entry.interval_duration(), Some(Duration::from_millis(40)));
    }

    #[test]
    fn duplicate_ids_are_rejected_before_reconciliation() {
        let id = SubscriptionId::singleton("duplicate");
        let subscriptions = Subscriptions::batch([
            Subscription::interval(id, Duration::from_secs(1), 1),
            Subscription::interval(id, Duration::from_secs(2), 2),
        ]);
        assert!(
            subscriptions
                .into_unique()
                .unwrap_err()
                .to_string()
                .contains("duplicate#0")
        );
    }

    #[test]
    #[should_panic(expected = "subscription interval must be non-zero")]
    fn zero_interval_is_rejected() {
        let _ = Subscription::interval(SubscriptionId::singleton("zero"), Duration::ZERO, ());
    }
}
