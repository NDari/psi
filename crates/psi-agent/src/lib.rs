//! The agent loop (psi-plan.md §5): a run repeats turns (one model request
//! plus the tool calls it returns) until the model stops calling tools or a
//! budget stops it.

mod events;
mod messages;
pub mod prompt;
mod run;

pub use events::AgentEvent;
pub use messages::build_messages;
pub use run::{Agent, AgentConfig, Budgets, Queues, RunOutcome, Stop};

// TODO(phase-3): guards (§8.6), policy decisions and checkpoints per step.
// TODO(phase-5): extension events (§9.4) from the same points as AgentEvent.
