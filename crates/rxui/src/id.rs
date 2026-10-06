use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) fn next_runtime() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
        .expect("RXUI runtime identity space exhausted")
}

/// Opaque entity identity, including runtime and slot generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityId {
    pub(crate) runtime: u64,
    pub(crate) slot: usize,
    pub(crate) generation: u64,
}

/// Opaque mount identity, independent of its model's identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MountId {
    pub(crate) runtime: u64,
    pub(crate) serial: u64,
}
