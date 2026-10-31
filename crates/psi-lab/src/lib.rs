//! Recording, inspection, replay and the eval runner (psi-plan.md §12).
//!
//! Library code never prints; long runs report progress through a callback.

pub mod adapter;
mod error;
pub mod eval;
mod fsutil;
pub mod inspect;

pub use error::{LabError, Result};
pub use fsutil::{now_ms, utc_stamp};

// TODO(phase-2): `psi replay` (§12.4). TODO(phase-7): `psi why` (§12.3).
