//! Reconciliation identity: keys inside a sequence, kinds across passes.

use std::{
    any::TypeId,
    cmp::Ordering as CmpOrdering,
    fmt,
    hash::{Hash, Hasher},
    sync::Arc,
};

use astrelis_ui_next::NodeId;
/// Stable view identity used by dynamic collections.
///
/// Keys are compared and hashed on every keyed reconciliation pass, so the
/// representation avoids allocating for the two idiomatic sources of identity: a
/// numeric domain id (`.key(item.id)`) and a `&'static str` literal
/// (`.key("header")`). Only an owned or borrowed non-static string allocates,
/// and then only once per key rather than once per frame.
///
/// String-shaped keys compare and hash by their text regardless of whether they
/// are borrowed or owned, so a collection keyed with a literal one frame and a
/// `String` the next keeps its retained identity. [`ViewKey::Index`] is a
/// distinct space: `ViewKey::from(7u64)` is *not* equal to `ViewKey::from("7")`.
#[derive(Clone, Debug)]
pub enum ViewKey {
    /// Numeric domain identity.
    Index(u64),
    /// Borrowed static identity, typically a literal.
    Name(&'static str),
    /// Shared owned identity.
    Owned(Arc<str>),
}

impl ViewKey {
    /// Creates an owned key.
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self::Owned(value.into())
    }

    /// Returns the textual form of a string-shaped key.
    fn text(&self) -> Option<&str> {
        match self {
            Self::Index(_) => None,
            Self::Name(value) => Some(value),
            Self::Owned(value) => Some(value),
        }
    }
}

impl PartialEq for ViewKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Index(left), Self::Index(right)) => left == right,
            (Self::Index(_), _) | (_, Self::Index(_)) => false,
            (left, right) => left.text() == right.text(),
        }
    }
}

impl Eq for ViewKey {}

impl Hash for ViewKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // The discriminant byte keeps the numeric and textual spaces apart; both
        // textual variants hash as `str` so they collide only when equal.
        match self {
            Self::Index(value) => {
                0u8.hash(state);
                value.hash(state);
            }
            Self::Name(value) => {
                1u8.hash(state);
                (*value).hash(state);
            }
            Self::Owned(value) => {
                1u8.hash(state);
                (**value).hash(state);
            }
        }
    }
}

impl PartialOrd for ViewKey {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for ViewKey {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        match (self, other) {
            (Self::Index(left), Self::Index(right)) => left.cmp(right),
            (Self::Index(_), _) => CmpOrdering::Less,
            (_, Self::Index(_)) => CmpOrdering::Greater,
            (left, right) => left.text().cmp(&right.text()),
        }
    }
}

impl From<&'static str> for ViewKey {
    fn from(value: &'static str) -> Self {
        Self::Name(value)
    }
}

impl From<String> for ViewKey {
    fn from(value: String) -> Self {
        Self::Owned(Arc::<str>::from(value))
    }
}

impl From<Arc<str>> for ViewKey {
    fn from(value: Arc<str>) -> Self {
        Self::Owned(value)
    }
}

impl From<u64> for ViewKey {
    fn from(value: u64) -> Self {
        Self::Index(value)
    }
}

impl From<NodeId> for ViewKey {
    fn from(value: NodeId) -> Self {
        Self::Index(value.to_bits())
    }
}

impl fmt::Display for ViewKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Index(value) => value.fmt(formatter),
            Self::Name(value) => value.fmt(formatter),
            Self::Owned(value) => value.fmt(formatter),
        }
    }
}

/// Type identity of one view kind.
///
/// Reconciliation pairs a fresh view with a mounted node only when their kinds
/// agree; a mismatch means the mounted subtree is removed and the new view is
/// mounted from scratch. The identity is the implementing Rust type, so two
/// distinct [`crate::ViewNode`] types never reconcile against each other even when
/// their mounted state happens to have the same shape - which is what makes
/// [`crate::Mounted::state_mut`] a lookup rather than a guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ViewKind(TypeId);

impl ViewKind {
    /// Returns the kind identifying `T`.
    pub fn of<T: ?Sized + 'static>() -> Self {
        Self(TypeId::of::<T>())
    }
}
