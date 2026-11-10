//! `psi eval` (psi-plan.md §12.5): coding tasks run in podman, graded by
//! hidden checks in a second container with no network.
//!
//! Baselines are single runs that prove the pipeline works end to end; they
//! are never a measure of speed or quality (§12.5, owner rule).

mod grade;
mod podman;
mod result;
mod run;
mod server;
mod summary;
mod tasks;
mod trial;

pub use grade::{Grade, grade};
pub use podman::{IMAGE, build_image};
pub use result::{Outcome, RunMeta, TrialResult, harness_failures};
pub use run::{CheckReport, CheckRow, RunConfig, check_graders, run};
pub use summary::{diff_runs, load_meta, load_results, summary_markdown};
pub use tasks::{Selection, Task, load_tasks};
#[doc(hidden)]
pub use trial::psi_outcome;
