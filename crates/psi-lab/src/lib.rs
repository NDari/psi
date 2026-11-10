//! Recording, inspection, replay and the eval runner (psi-plan.md §12).
//!
//! Library code never prints; long runs report progress through a callback.

mod error;
pub mod eval;
mod fsutil;
pub mod guards;
pub mod inspect;
pub mod replay;

pub use error::{LabError, Result};
pub use psi_session::time::{now_ms, utc_stamp};
