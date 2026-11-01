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
pub use jsonl::{SessionError, read_entries, read_entries_lossy, write_entries};
pub use store::{SessionFile, branch, cwd_slug};

// TODO(phase-7): /fork and /export (HTML) alongside the export work.
// Checkpoints themselves live in psi-safety; sessions record them as entries.
// TODO(phase-7): compaction tuning (§7.4).
