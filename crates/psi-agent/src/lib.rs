//! The agent loop (psi-plan.md §5): a run repeats turns (one model request
//! plus the tool calls it returns) until the model stops calling tools or a
//! budget stops it.

pub mod approval;
mod events;
pub mod guards;
mod messages;
pub mod prompt;
pub mod recovery;
mod run;

pub use events::AgentEvent;
pub use messages::build_messages;
pub use run::{Agent, AgentConfig, Budgets, Queues, RunOutcome, Safety, Stop};

// TODO(phase-5): extension events (§9.4) from the same points as AgentEvent.
