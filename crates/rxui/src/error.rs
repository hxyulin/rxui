use std::{error::Error, fmt};

/// Invalid entity or mount access. Safe access never aliases a mutable value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessError {
    /// The handle belongs to another runtime.
    WrongRuntime,
    /// The value or mount has been disposed.
    Disposed,
    /// The value is already being read or updated incompatibly.
    Borrowed,
    /// The mount is already being evaluated.
    Evaluating,
}
impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::WrongRuntime => "handle belongs to another RXUI runtime",
            Self::Disposed => "RXUI value or mount has been disposed",
            Self::Borrowed => "RXUI entity is already borrowed; use the supplied state reference",
            Self::Evaluating => "RXUI mount is already being evaluated",
        })
    }
}
impl Error for AccessError {}

/// Deferred callbacks produced an unbounded notification/effect cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectCycle;
impl fmt::Display for EffectCycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RXUI deferred effects exceeded the per-flush limit; possible observer cycle")
    }
}
impl Error for EffectCycle {}
