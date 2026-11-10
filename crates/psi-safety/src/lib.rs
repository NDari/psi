//! Safety (design-going-forward.md, goal 1): undo (checkpoints), a boundary
//! (isolation, with credentials hidden inside it) and a short refuse list.

pub mod blocked;
pub mod checkpoint;
pub mod git;
pub mod isolation;
pub mod last_command;
pub mod policy;
pub mod refuse;
pub mod secrets;

pub use isolation::{Isolation, Sandbox, SandboxConfig};
