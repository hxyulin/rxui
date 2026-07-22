//! Shared ordered application-message queue semantics.

use std::collections::{HashMap, VecDeque};

use crate::{InstrumentationState, MessageMetadata, MessageOrigin, QueuedMessage, WindowId};

/// Maximum passes over messages posted from an update before deferral.
#[doc(hidden)]
pub const POSTED_PASS_LIMIT: usize = 8;

/// A runner-local identity used to replace an older pending message with its
/// newest value.
///
/// Namespaces should describe the coalesced event, such as
/// `"chart.viewport"`. Use the instance component to distinguish repeated
/// feature instances.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MessageKey {
    namespace: &'static str,
    instance: u64,
}

impl MessageKey {
    /// Creates a key in a static namespace for one stable feature instance.
    pub const fn new(namespace: &'static str, instance: u64) -> Self {
        Self {
            namespace,
            instance,
        }
    }

    /// Creates a key for an application-wide singleton event.
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

/// Shared queue kernel used by the native runner and deterministic harness.
#[doc(hidden)]
pub struct PostedQueue<M> {
    entries: VecDeque<QueuedMessage<M>>,
    keyed: HashMap<MessageKey, usize>,
    replacements: u64,
}

impl<M> Default for PostedQueue<M> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            keyed: HashMap::new(),
            replacements: 0,
        }
    }
}

impl<M> PostedQueue<M> {
    /// Appends one ordinary FIFO message.
    #[doc(hidden)]
    pub fn post(&mut self, message: QueuedMessage<M>) {
        self.entries.push_back(message);
    }

    /// Replaces a pending keyed message while preserving its queue position.
    #[doc(hidden)]
    pub fn replace_latest(
        &mut self,
        key: MessageKey,
        message: M,
        source: Option<WindowId>,
        metadata: MessageMetadata,
        origin: MessageOrigin,
        instrumentation: &mut InstrumentationState,
    ) -> Result<(), M> {
        if let Some(index) = self.keyed.get(&key).copied() {
            let entry = self
                .entries
                .get_mut(index)
                .expect("pending keyed-message index stays valid until the batch is drained");
            entry.replace(message, source, metadata, origin);
            instrumentation.coalesce_message(entry);
            self.replacements = self.replacements.saturating_add(1);
            return Ok(());
        }
        Err(message)
    }

    /// Appends a new keyed message.
    #[doc(hidden)]
    pub fn post_keyed(&mut self, key: MessageKey, message: QueuedMessage<M>) {
        let index = self.entries.len();
        self.entries.push_back(message);
        self.keyed.insert(key, index);
    }

    /// Drains one dispatch batch and clears replaceable indices.
    #[doc(hidden)]
    pub fn take(&mut self) -> Vec<QueuedMessage<M>> {
        self.keyed.clear();
        self.entries.drain(..).collect()
    }

    /// Returns whether no messages are pending.
    #[doc(hidden)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the pending queue depth.
    #[doc(hidden)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns cumulative latest-value replacements.
    #[doc(hidden)]
    pub const fn replacements(&self) -> u64 {
        self.replacements
    }
}
