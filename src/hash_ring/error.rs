//! Configuration errors returned when constructing a ring.

use std::fmt;

/// An invalid hash-ring configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingError {
    /// Every physical node needs at least one virtual position.
    ZeroVirtualNodes,
}

impl fmt::Display for RingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroVirtualNodes => {
                f.write_str("virtual nodes per node must be greater than zero")
            }
        }
    }
}

impl std::error::Error for RingError {}
