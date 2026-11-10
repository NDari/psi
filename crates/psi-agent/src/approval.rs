//! Who is at the keyboard (design-going-forward.md: nothing asks before a
//! call). What is left to answer is whether to go on after a guard's pause
//! or a budget: the TUI shows a card, headless runs stop.

use futures_util::future::BoxFuture;

pub trait Approver: Send + Sync {
    /// A guard paused the run; true goes on.
    fn pause(&self, reason: String) -> BoxFuture<'static, bool>;
    /// Whether a person answers.
    fn interactive(&self) -> bool;
}

/// Print, JSON and eval runs: nothing prompts, and a pause stops.
#[derive(Debug, Clone, Copy, Default)]
pub struct Headless;

impl Approver for Headless {
    fn pause(&self, _: String) -> BoxFuture<'static, bool> {
        Box::pin(async { false })
    }
    fn interactive(&self) -> bool {
        false
    }
}
