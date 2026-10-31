//! Always-ask detection, modes, the isolation backend and host-git flags (psi-plan.md §8).
//!
//! Three cheap mechanisms (D-30): undo (checkpoints), a boundary
//! (isolation) and a gate (the always-ask list).

pub mod always_ask;
pub mod blocked;
pub mod checkpoint;
pub mod git;
pub mod isolation;
pub mod policy;

pub use isolation::{Isolation, Sandbox, SandboxConfig};
