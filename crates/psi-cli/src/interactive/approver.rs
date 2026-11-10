//! Pauses in the TUI: the agent's question whether to go on goes to the UI
//! as a card under the input box, and the answer comes back.

use futures_util::future::BoxFuture;
use psi_agent::approval::Approver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use super::ui::UiMsg;

pub struct TuiApprover {
    pub ui: UnboundedSender<UiMsg>,
}

impl Approver for TuiApprover {
    fn pause(&self, reason: String) -> BoxFuture<'static, bool> {
        let (tx, rx) = oneshot::channel();
        let sent = self.ui.send(UiMsg::Pause(reason, tx)).is_ok();
        Box::pin(async move { sent && rx.await.unwrap_or(false) })
    }

    fn interactive(&self) -> bool {
        true
    }
}
