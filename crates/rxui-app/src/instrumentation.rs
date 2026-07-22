//! Opt-in, payload-free runtime instrumentation.

use std::{collections::VecDeque, time::Duration};

use crate::{
    MessageKey, SubscriptionId, SubscriptionKind, SubscriptionStatus, TaskId, TaskKind, TimerId,
    WindowId,
};

/// Static, payload-free identity supplied by an application for one message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageMetadata {
    name: &'static str,
    category: &'static str,
    scope: &'static str,
}

impl MessageMetadata {
    /// Creates metadata with the default `application` category.
    pub const fn named(name: &'static str) -> Self {
        Self::new(name, "application")
    }

    /// Creates metadata with an explicit name and category.
    pub const fn new(name: &'static str, category: &'static str) -> Self {
        Self {
            name,
            category,
            scope: "application",
        }
    }

    /// Associates this message with a diagnostic scope.
    pub const fn in_scope(mut self, scope: &'static str) -> Self {
        self.scope = scope;
        self
    }

    /// Returns metadata for an application that has not named its messages.
    pub const fn unnamed() -> Self {
        Self::named("<unnamed>")
    }

    /// Returns the diagnostic message name.
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Returns the diagnostic category.
    pub const fn category(self) -> &'static str {
        self.category
    }

    /// Returns the diagnostic ownership scope.
    pub const fn scope(self) -> &'static str {
        self.scope
    }
}

impl Default for MessageMetadata {
    fn default() -> Self {
        Self::unnamed()
    }
}

/// Runtime source that caused a message to enter application dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MessageOrigin {
    /// Emitted by a retained UI event listener.
    Ui,
    /// Posted from an application update through `AppCx`.
    Posted,
    /// Submitted through a cross-thread message proxy.
    Proxy,
    /// Produced by a one-shot timer.
    Timeout(TimerId),
    /// Produced by an imperative repeating timer.
    Interval(TimerId),
    /// Produced by a completed background task.
    Task(TaskId),
    /// Produced by a declarative subscription.
    Subscription(SubscriptionId),
    /// Injected directly by an alternative backend such as a test harness.
    External,
}

/// Whether an application update completed successfully.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageOutcome {
    /// The update returned successfully.
    Success,
    /// The update returned an error. Error text is never retained.
    Error,
}

/// Runtime-owned resource described by a lifecycle trace.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuntimeResource {
    /// One application-scoped background task.
    Task {
        /// Application-local task identity.
        id: TaskId,
        /// Application-provided diagnostic name.
        name: String,
        /// Execution strategy used by the task.
        kind: TaskKind,
        /// Diagnostic scope inherited from the message that started the task.
        scope: &'static str,
    },
    /// One declarative subscription generation.
    Subscription {
        /// Stable desired-state identity.
        id: SubscriptionId,
        /// Framework-owned source kind.
        kind: SubscriptionKind,
        /// Running or startup-failed state.
        status: SubscriptionStatus,
    },
}

/// State transition recorded for a runtime-owned resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuntimeLifecycleEvent {
    /// A new resource started.
    Started,
    /// Changed subscription configuration replaced an older generation.
    Restarted,
    /// A task delivered its completion.
    Completed,
    /// Explicit cancellation or desired-state removal stopped the resource.
    Cancelled,
    /// Dropping an unfinished task completion abandoned the task.
    Abandoned,
    /// Resource startup failed.
    Failed,
}

/// One bounded, payload-free runtime resource lifecycle record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLifecycleTrace {
    sequence: u64,
    resource: RuntimeResource,
    event: RuntimeLifecycleEvent,
    lifetime: Duration,
}

impl RuntimeLifecycleTrace {
    /// Returns the monotonically increasing runtime event sequence.
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Returns the task or subscription identity and payload-free metadata.
    pub const fn resource(&self) -> &RuntimeResource {
        &self.resource
    }

    /// Returns the recorded lifecycle transition.
    pub const fn event(&self) -> RuntimeLifecycleEvent {
        self.event
    }

    /// Returns resource age when the transition occurred.
    pub const fn lifetime(&self) -> Duration {
        self.lifetime
    }
}

/// Immutable identity shared by events for one message delivery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageTraceIdentity {
    sequence: u64,
    metadata: MessageMetadata,
    source: Option<WindowId>,
    origin: MessageOrigin,
}

impl MessageTraceIdentity {
    /// Returns the monotonically increasing application-local sequence.
    pub const fn sequence(self) -> u64 {
        self.sequence
    }

    /// Returns the application-supplied payload-free metadata.
    pub const fn metadata(self) -> MessageMetadata {
        self.metadata
    }

    /// Returns the source window, when one was associated with delivery.
    pub const fn source(self) -> Option<WindowId> {
        self.source
    }

    /// Returns the runtime delivery origin.
    pub const fn origin(self) -> MessageOrigin {
        self.origin
    }
}

/// One completed, payload-free application message dispatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageTrace {
    identity: MessageTraceIdentity,
    queue_depth: usize,
    queue_latency: Duration,
    update_duration: Duration,
    key: Option<MessageKey>,
    replacements: u64,
    emitted: u64,
    outcome: MessageOutcome,
}

impl MessageTrace {
    /// Returns the message identity.
    pub const fn identity(&self) -> MessageTraceIdentity {
        self.identity
    }

    /// Returns queue depth immediately after this message was accepted.
    pub const fn queue_depth(&self) -> usize {
        self.queue_depth
    }

    /// Returns elapsed time between RXUI queue ingress and dispatch.
    pub const fn queue_latency(&self) -> Duration {
        self.queue_latency
    }

    /// Returns elapsed time spent in `App::update`.
    pub const fn update_duration(&self) -> Duration {
        self.update_duration
    }

    /// Returns the latest-value key, when coalescing was requested.
    pub const fn key(&self) -> Option<MessageKey> {
        self.key
    }

    /// Returns the number of pending values replaced before dispatch.
    pub const fn replacements(&self) -> u64 {
        self.replacements
    }

    /// Returns messages emitted while this update ran.
    pub const fn emitted(&self) -> u64 {
        self.emitted
    }

    /// Returns whether the application update succeeded.
    pub const fn outcome(&self) -> MessageOutcome {
        self.outcome
    }
}

/// Payload-free event delivered to a [`RuntimeObserver`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuntimeEvent {
    /// A message entered RXUI's dispatch queue.
    MessageQueued {
        /// Message identity.
        identity: MessageTraceIdentity,
        /// Queue depth immediately after insertion.
        queue_depth: usize,
        /// Latest-value key, when present.
        key: Option<MessageKey>,
    },
    /// A pending latest-value message was replaced.
    MessageCoalesced {
        /// Identity updated to describe the newest value.
        identity: MessageTraceIdentity,
        /// Latest-value key.
        key: MessageKey,
        /// Number of replacements accumulated by the entry.
        replacements: u64,
    },
    /// Dispatch into `App::update` began.
    MessageDispatchStarted {
        /// Message identity.
        identity: MessageTraceIdentity,
        /// Time spent waiting in RXUI's queue.
        queue_latency: Duration,
    },
    /// Dispatch into `App::update` finished.
    MessageDispatchFinished(MessageTrace),
    /// A task or subscription changed lifecycle state.
    LifecycleRecorded(RuntimeLifecycleTrace),
}

/// Read-only sink for payload-free runtime events.
pub trait RuntimeObserver: 'static {
    /// Observes one event synchronously on the application thread.
    fn observe(&mut self, event: &RuntimeEvent);
}

impl<F> RuntimeObserver for F
where
    F: FnMut(&RuntimeEvent) + 'static,
{
    fn observe(&mut self, event: &RuntimeEvent) {
        self(event);
    }
}

/// Opt-in runtime instrumentation configuration.
#[derive(Default)]
pub struct RuntimeInstrumentationConfig {
    pub(crate) message_history_capacity: usize,
    pub(crate) lifecycle_history_capacity: usize,
    pub(crate) observer: Option<Box<dyn RuntimeObserver>>,
}

impl RuntimeInstrumentationConfig {
    /// Retains at most `capacity` completed message traces.
    pub const fn message_history(mut self, capacity: usize) -> Self {
        self.message_history_capacity = capacity;
        self
    }

    /// Retains at most `capacity` task and subscription lifecycle traces.
    pub const fn lifecycle_history(mut self, capacity: usize) -> Self {
        self.lifecycle_history_capacity = capacity;
        self
    }

    /// Installs a payload-free event observer.
    pub fn observer(mut self, observer: impl RuntimeObserver) -> Self {
        self.observer = Some(Box::new(observer));
        self
    }

    pub(crate) fn enabled(&self) -> bool {
        self.message_history_capacity > 0
            || self.lifecycle_history_capacity > 0
            || self.observer.is_some()
    }
}

#[doc(hidden)]
pub struct PendingMessageTrace {
    identity: MessageTraceIdentity,
    queued_at: crate::Instant,
    queue_depth: usize,
    key: Option<MessageKey>,
    replacements: u64,
    started_at: Option<crate::Instant>,
    emissions_at_start: u64,
}

impl PendingMessageTrace {
    pub(crate) fn update_latest(
        &mut self,
        metadata: MessageMetadata,
        source: Option<WindowId>,
        origin: MessageOrigin,
    ) {
        self.identity.metadata = metadata;
        self.identity.source = source;
        self.identity.origin = origin;
        self.replacements = self.replacements.saturating_add(1);
    }
}

#[doc(hidden)]
pub struct InstrumentationState {
    enabled: bool,
    history_capacity: usize,
    history: VecDeque<MessageTrace>,
    lifecycle_capacity: usize,
    lifecycle: VecDeque<RuntimeLifecycleTrace>,
    observer: Option<Box<dyn RuntimeObserver>>,
    next_sequence: u64,
    emissions: u64,
    replacements: u64,
    current_scope: Option<&'static str>,
}

impl InstrumentationState {
    pub fn new(config: RuntimeInstrumentationConfig) -> Self {
        Self {
            enabled: config.enabled(),
            history_capacity: config.message_history_capacity,
            history: VecDeque::with_capacity(config.message_history_capacity),
            lifecycle_capacity: config.lifecycle_history_capacity,
            lifecycle: VecDeque::with_capacity(config.lifecycle_history_capacity),
            observer: config.observer,
            next_sequence: 1,
            emissions: 0,
            replacements: 0,
            current_scope: None,
        }
    }

    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn queued(
        &mut self,
        metadata: MessageMetadata,
        source: Option<WindowId>,
        origin: MessageOrigin,
        queued_at: crate::Instant,
        queue_depth: usize,
        key: Option<MessageKey>,
    ) -> Option<PendingMessageTrace> {
        if !self.enabled {
            return None;
        }
        let identity = MessageTraceIdentity {
            sequence: self.next_sequence,
            metadata,
            source,
            origin,
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.emissions = self.emissions.saturating_add(1);
        self.notify(RuntimeEvent::MessageQueued {
            identity,
            queue_depth,
            key,
        });
        Some(PendingMessageTrace {
            identity,
            queued_at,
            queue_depth,
            key,
            replacements: 0,
            started_at: None,
            emissions_at_start: 0,
        })
    }

    pub(crate) fn coalesced(&mut self, trace: &mut Option<PendingMessageTrace>) {
        if let Some(trace) = trace {
            self.emissions = self.emissions.saturating_add(1);
            self.replacements = self.replacements.saturating_add(1);
            self.notify(RuntimeEvent::MessageCoalesced {
                identity: trace.identity,
                key: trace.key.expect("coalesced trace has a key"),
                replacements: trace.replacements,
            });
        }
    }

    pub fn started(&mut self, trace: &mut Option<PendingMessageTrace>, now: crate::Instant) {
        let Some(trace) = trace else { return };
        trace.started_at = Some(now);
        trace.emissions_at_start = self.emissions;
        self.current_scope = Some(trace.identity.metadata.scope());
        self.notify(RuntimeEvent::MessageDispatchStarted {
            identity: trace.identity,
            queue_latency: now.saturating_duration_since(trace.queued_at),
        });
    }

    pub fn finished(
        &mut self,
        trace: Option<PendingMessageTrace>,
        now: crate::Instant,
        outcome: MessageOutcome,
    ) {
        let Some(trace) = trace else { return };
        let started_at = trace.started_at.unwrap_or(trace.queued_at);
        let completed = MessageTrace {
            identity: trace.identity,
            queue_depth: trace.queue_depth,
            queue_latency: started_at.saturating_duration_since(trace.queued_at),
            update_duration: now.saturating_duration_since(started_at),
            key: trace.key,
            replacements: trace.replacements,
            emitted: self.emissions.saturating_sub(trace.emissions_at_start),
            outcome,
        };
        self.current_scope = None;
        self.notify(RuntimeEvent::MessageDispatchFinished(completed.clone()));
        if self.history_capacity > 0 {
            if self.history.len() == self.history_capacity {
                self.history.pop_front();
            }
            self.history.push_back(completed);
        }
    }

    pub fn record_lifecycle(
        &mut self,
        resource: RuntimeResource,
        event: RuntimeLifecycleEvent,
        lifetime: Duration,
    ) {
        if !self.enabled {
            return;
        }
        let trace = RuntimeLifecycleTrace {
            sequence: self.next_sequence,
            resource,
            event,
            lifetime,
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.notify(RuntimeEvent::LifecycleRecorded(trace.clone()));
        if self.lifecycle_capacity > 0 {
            if self.lifecycle.len() == self.lifecycle_capacity {
                self.lifecycle.pop_front();
            }
            self.lifecycle.push_back(trace);
        }
    }

    /// Returns the scope of the message currently being dispatched.
    #[doc(hidden)]
    pub const fn current_scope(&self) -> Option<&'static str> {
        self.current_scope
    }

    pub fn snapshot(&self) -> (Vec<MessageTrace>, Vec<RuntimeLifecycleTrace>, u64) {
        (
            self.history.iter().cloned().collect(),
            self.lifecycle.iter().cloned().collect(),
            self.replacements,
        )
    }

    fn notify(&mut self, event: RuntimeEvent) {
        if let Some(observer) = self.observer.as_mut() {
            observer.observe(&event);
        }
    }
}

#[doc(hidden)]
pub struct QueuedMessage<M> {
    pub(crate) message: M,
    pub(crate) source: Option<WindowId>,
    pub(crate) trace: Option<PendingMessageTrace>,
}

impl<M> QueuedMessage<M> {
    /// Creates an optionally instrumented queue envelope.
    #[doc(hidden)]
    pub fn new(message: M, source: Option<WindowId>, trace: Option<PendingMessageTrace>) -> Self {
        Self {
            message,
            source,
            trace,
        }
    }

    /// Splits the payload, source, and opaque dispatch state.
    #[doc(hidden)]
    pub fn into_parts(self) -> (M, Option<WindowId>, MessageDispatch) {
        (
            self.message,
            self.source,
            MessageDispatch { trace: self.trace },
        )
    }

    /// Replaces the pending value while preserving queue position and age.
    #[doc(hidden)]
    pub fn replace(
        &mut self,
        message: M,
        source: Option<WindowId>,
        metadata: MessageMetadata,
        origin: MessageOrigin,
    ) {
        self.message = message;
        self.source = source;
        if let Some(trace) = &mut self.trace {
            trace.update_latest(metadata, source, origin);
        }
    }
}

/// Opaque instrumentation state carried while one message is dispatched.
#[doc(hidden)]
pub struct MessageDispatch {
    pub(crate) trace: Option<PendingMessageTrace>,
}

impl InstrumentationState {
    /// Marks one queued envelope as dispatched.
    #[doc(hidden)]
    pub fn start_message(&mut self, message: &mut MessageDispatch, now: crate::Instant) {
        self.started(&mut message.trace, now);
    }

    /// Records replacement of one latest-value queue envelope.
    #[doc(hidden)]
    pub fn coalesce_message<M>(&mut self, message: &mut QueuedMessage<M>) {
        self.coalesced(&mut message.trace);
    }

    /// Completes one opaque dispatch state.
    #[doc(hidden)]
    pub fn finish_message(
        &mut self,
        message: MessageDispatch,
        now: crate::Instant,
        outcome: MessageOutcome,
    ) {
        self.finished(message.trace, now, outcome);
    }
}
