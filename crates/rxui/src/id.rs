use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) fn next_runtime() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    next_identity(&NEXT)
}

// A checked allocator compatible with the declared MSRV. fetch_update was renamed
// in Rust 1.99; compare_exchange_weak avoids either a deprecated API or a newer MSRV.
pub(crate) fn next_identity(counter: &AtomicU64) -> u64 {
    let mut value = counter.load(Ordering::Relaxed);
    loop {
        let next = value.checked_add(1).expect("RXUI identity space exhausted");
        match counter.compare_exchange_weak(value, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return value,
            Err(actual) => value = actual,
        }
    }
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
