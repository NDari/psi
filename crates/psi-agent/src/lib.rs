//! The agent loop (psi-plan.md §5): a run repeats turns (one model request
//! plus the tool calls it returns) until the model stops calling tools or a
//! budget stops it.

pub mod approval;
mod compact;
mod events;
pub mod ext;
pub mod guards;
mod messages;
pub mod plan;
pub mod post_edit;
pub mod prompt;
pub mod recovery;
mod run;
pub mod services;
pub mod subagent;

pub use compact::{CompactionSettings, SummaryForm};
pub use events::AgentEvent;
pub use messages::build_messages;
pub use post_edit::PostEditCheck;
pub use run::{Agent, AgentConfig, Budgets, ConfigWriteHook, Queues, RunOutcome, Safety, Stop};
