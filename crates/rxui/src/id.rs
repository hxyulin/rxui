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

/// Map keyed by RXUI's own identities, hashed with [`IdHasher`].
pub(crate) type IdMap<K, V> =
    std::collections::HashMap<K, V, std::hash::BuildHasherDefault<IdHasher>>;
/// Set of RXUI's own identities, hashed with [`IdHasher`].
pub(crate) type IdSet<K> = std::collections::HashSet<K, std::hash::BuildHasherDefault<IdHasher>>;

/// Multiply-rotate hasher (the Fx hash) for identities RXUI allocates itself.
/// Keys are never chosen by input, so SipHash's flooding resistance buys nothing;
/// node and resource lookups dominated frame preparation with it.
#[derive(Clone, Copy, Default)]
pub(crate) struct IdHasher(u64);
impl std::hash::Hasher for IdHasher {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }
    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }
    fn finish(&self) -> u64 {
        // The table indexes by low bits; the product's best-mixed bits are high.
        self.0.rotate_left(26)
    }
}
