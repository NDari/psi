//! Approvals in the TUI (psi-plan.md §8.7): the agent's question goes to
//! the UI as a card under the input box, and the answer comes back.

use futures_util::future::BoxFuture;
use psi_agent::approval::{Approval, ApprovalRequest, Approver, BlockedChoice, BlockedRequest};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use super::ui::UiMsg;

pub struct TuiApprover {
    pub ui: UnboundedSender<UiMsg>,
}

impl Approver for TuiApprover {
    fn approve(&self, req: ApprovalRequest) -> BoxFuture<'static, Approval> {
        let (tx, rx) = oneshot::channel();
        let sent = self.ui.send(UiMsg::Approve(req, tx)).is_ok();
        // A UI that is gone can't say yes.
        Box::pin(async move {
            if !sent {
                return Approval::Deny;
            }
            rx.await.unwrap_or(Approval::Deny)
        })
    }

    fn blocked(&self, req: BlockedRequest) -> BoxFuture<'static, BlockedChoice> {
        let (tx, rx) = oneshot::channel();
        let sent = self.ui.send(UiMsg::Blocked(req, tx)).is_ok();
        Box::pin(async move {
            if !sent {
                return BlockedChoice::No;
            }
            rx.await.unwrap_or(BlockedChoice::No)
        })
    }

    fn pause(&self, reason: String) -> BoxFuture<'static, bool> {
        let (tx, rx) = oneshot::channel();
        let sent = self.ui.send(UiMsg::Pause(reason, tx)).is_ok();
        Box::pin(async move { sent && rx.await.unwrap_or(false) })
    }

    fn interactive(&self) -> bool {
        true
    }
}
