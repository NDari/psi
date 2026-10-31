//! JSONL tree sessions, compaction and checkpoints (psi-plan.md §7).
//!
//! The session file plus its wire log is the only record of a run (D-31):
//! live sessions, eval trials and other agents' output converted by an
//! adapter all use these entry types.

pub mod entry;
pub mod jsonl;
pub mod store;
pub mod time;

pub use entry::{Block, Entry, EntryBody, IdGen, Role, StopReason, Usage, text_of};
pub use jsonl::{SessionError, read_entries, write_entries};
pub use store::{SessionFile, branch, cwd_slug};

// TODO(phase-4): /tree, /fork and /label in the TUI.
// TODO(phase-3): checkpoints (§7.2). TODO(phase-7): compaction tuning (§7.4).
