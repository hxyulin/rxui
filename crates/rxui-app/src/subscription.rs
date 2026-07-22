//! Declarative descriptions of application-scoped long-lived event sources.

use std::{collections::HashSet, fmt, rc::Rc, time::Duration};

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
}

/// Read-only metadata for one currently active subscription.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveSubscriptionSnapshot {
    id: SubscriptionId,
    kind: SubscriptionKind,
    delivery: DeliveryPolicy,
    interval: Duration,
    elapsed: Duration,
    starts: u64,
}

impl ActiveSubscriptionSnapshot {
    /// Creates subscription metadata for an alternative backend.
    #[doc(hidden)]
    pub const fn new(
        id: SubscriptionId,
        kind: SubscriptionKind,
        delivery: DeliveryPolicy,
        interval: Duration,
        elapsed: Duration,
        starts: u64,
    ) -> Self {
        Self {
            id,
            kind,
            delivery,
            interval,
            elapsed,
            starts,
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
    /// Returns the repeating interval.
    pub const fn interval(&self) -> Duration {
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
}

/// Structural configuration used to decide whether a source restarts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[doc(hidden)]
pub struct SubscriptionConfig {
    pub(crate) kind: SubscriptionKind,
    pub(crate) interval: Duration,
    pub(crate) delivery: DeliveryPolicy,
}

impl SubscriptionConfig {
    /// Returns the source category.
    #[doc(hidden)]
    pub const fn kind(self) -> SubscriptionKind {
        self.kind
    }
    /// Returns the interval duration.
    #[doc(hidden)]
    pub const fn interval(self) -> Duration {
        self.interval
    }
    /// Returns the delivery policy.
    #[doc(hidden)]
    pub const fn delivery(self) -> DeliveryPolicy {
        self.delivery
    }
}

/// One desired application-scoped event source.
pub struct Subscription<M: 'static> {
    pub(crate) id: SubscriptionId,
    pub(crate) config: SubscriptionConfig,
    pub(crate) factory: Box<dyn FnMut() -> M>,
}

impl<M: 'static> Subscription<M> {
    /// Creates a repeating interval from one cloneable message.
    ///
    /// Missed intervals and pending ticks default to latest-value delivery.
    /// The interval must be non-zero.
    pub fn interval(id: SubscriptionId, interval: Duration, message: M) -> Self
    where
        M: Clone,
    {
        Self::interval_with(id, interval, move || message.clone())
    }

    /// Creates a repeating interval whose factory runs on the UI thread.
    ///
    /// The interval must be non-zero.
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
                interval,
                delivery: DeliveryPolicy::Latest,
            },
            factory: Box::new(factory),
        }
    }

    /// Changes how pending events from this source are delivered.
    #[must_use]
    pub const fn delivery(mut self, delivery: DeliveryPolicy) -> Self {
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

    /// Returns the repeating period for an interval source.
    pub const fn interval_duration(&self) -> Duration {
        self.config.interval
    }

    /// Decomposes a description for an alternative application backend.
    #[doc(hidden)]
    pub fn into_parts(self) -> (SubscriptionId, SubscriptionConfig, Box<dyn FnMut() -> M>) {
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
    pub(crate) entries: Vec<Subscription<M>>,
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
                    let mut factory = subscription.factory;
                    let map = Rc::clone(&map);
                    Subscription {
                        id: subscription.id,
                        config: subscription.config,
                        factory: Box::new(move || map(factory())),
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
        let mut entry = entries.pop().unwrap();
        assert_eq!(entry.id, SubscriptionId::new("editor.poll", 7));
        assert_eq!(entry.config.interval, Duration::from_millis(40));
        assert_eq!((entry.factory)(), "3");
    }

    #[test]
    fn duplicate_ids_are_rejected_before_reconciliation() {
        let id = SubscriptionId::singleton("duplicate");
        let subscriptions = Subscriptions::batch([
            Subscription::interval(id, Duration::from_secs(1), 1),
            Subscription::interval(id, Duration::from_secs(2), 2),
        ]);
        let error = subscriptions.into_unique().unwrap_err();
        assert!(error.to_string().contains("duplicate#0"));
    }

    #[test]
    #[should_panic(expected = "subscription interval must be non-zero")]
    fn zero_interval_is_rejected() {
        let _ = Subscription::interval(SubscriptionId::singleton("zero"), Duration::ZERO, ());
    }
}
