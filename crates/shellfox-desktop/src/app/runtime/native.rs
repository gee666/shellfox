use super::super::{Backend, TerminalView};
use super::{Outbox, intent::Intent, retire::Retirement};
use crate::host_client::HostClient;
use anyhow::{Context as _, Result, ensure};
use gpui::Context;
use shellfox_core::TabId;
use shellfox_runtime::{Command, Lease, Response, SubmitError};
use shellfox_service::{
    Attachment,
    native::{
        NativeAction, NativeAdmissionError, NativeCommand, NativeInputContext,
        NativeObserverAttachment, NativePresentation, NativeResponse, NativeViewAttachment,
    },
};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Clone)]
pub(in crate::app) enum Access {
    Control(Attachment),
    Observer(NativeObserverAttachment),
}
impl Access {
    pub fn lease(&self) -> Option<Lease> {
        match self {
            Self::Control(a) => Some(Lease {
                controller: a.controller,
                epoch: a.epoch,
            }),
            Self::Observer(_) => None,
        }
    }
    pub fn presentation_lease(&self) -> shellfox_runtime::PresentationLease {
        match self {
            Self::Control(a) => shellfox_runtime::PresentationLease::Controller(Lease {
                controller: a.controller,
                epoch: a.epoch,
            }),
            Self::Observer(a) => {
                shellfox_runtime::PresentationLease::Observer(shellfox_runtime::ObserverLease {
                    viewer: a.viewer,
                    epoch: a.epoch,
                })
            }
        }
    }
    fn view_attachment(&self) -> NativeViewAttachment {
        match self {
            Self::Control(a) => NativeViewAttachment::Controller(a.clone()),
            Self::Observer(a) => NativeViewAttachment::Observer(a.clone()),
        }
    }
    fn detach(&self, tab: TabId) -> NativeAction {
        match self {
            Self::Control(a) => NativeAction::Detach {
                tab,
                attachment: a.clone(),
            },
            Self::Observer(a) => NativeAction::Unobserve {
                tab,
                attachment: a.clone(),
            },
        }
    }
    #[cfg(test)]
    fn action(&self, tab: TabId, command: Command) -> Result<NativeAction> {
        let input = super::input_context::input_command(&command).then_some(NativeInputContext {
            captured_settlement: shellfox_host::CapturedSettlement::Captured(None),
            user_initiated: true,
            hint: true,
        });
        self.action_with_input(tab, command, input)
    }
    fn action_with_input(
        &self,
        tab: TabId,
        command: Command,
        input: Option<NativeInputContext>,
    ) -> Result<NativeAction> {
        if matches!(command, Command::RetryClose | Command::Close { .. }) {
            return Ok(self.detach(tab));
        }
        if let Command::Presentation { view, action } = command {
            ensure!(
                view == self.presentation_lease() && input.is_none(),
                "Queued presentation belongs to a stale view or carries input context"
            );
            let action = match action {
                shellfox_runtime::PresentationAction::Frame { focused } => {
                    NativePresentation::Frame { focused }
                }
                shellfox_runtime::PresentationAction::Scroll { delta } => {
                    NativePresentation::Scroll { delta }
                }
                shellfox_runtime::PresentationAction::FollowTail => NativePresentation::FollowTail,
                shellfox_runtime::PresentationAction::ResizeExternal { cols, rows } => {
                    NativePresentation::ResizeExternal { cols, rows }
                }
                shellfox_runtime::PresentationAction::Selection { selection } => {
                    NativePresentation::Selection { selection }
                }
                shellfox_runtime::PresentationAction::SelectionAtVersion { version, selection } => {
                    NativePresentation::SelectionAtVersion { version, selection }
                }
                shellfox_runtime::PresentationAction::Copy => NativePresentation::Copy,
                shellfox_runtime::PresentationAction::Hyperlink { point } => {
                    NativePresentation::Hyperlink { point }
                }
            };
            return Ok(NativeAction::Presentation {
                tab,
                attachment: self.view_attachment(),
                action,
            });
        }
        if let Self::Observer(attachment) = self {
            if let Command::ObserverRecover { observer, .. } = command {
                ensure!(
                    observer.viewer == attachment.viewer && observer.epoch == attachment.epoch,
                    "Stale observer intent"
                );
                return Ok(NativeAction::RecoverObserver {
                    tab,
                    attachment: attachment.clone(),
                });
            }
            anyhow::bail!("This terminal view is read-only");
        }
        let Self::Control(attachment) = self else {
            unreachable!("observer handled above")
        };
        let expected = self.lease().context("Control lease is missing")?;
        let (lease, command) = match command {
            Command::Key { lease, event } => (lease, NativeCommand::Key(event)),
            Command::Paste { lease, text } => (lease, NativeCommand::Paste(text)),
            Command::Mouse {
                lease,
                event,
                geometry,
            } => (lease, NativeCommand::Mouse { event, geometry }),
            Command::Resize { lease, cols, rows } => (lease, NativeCommand::Resize { cols, rows }),
            Command::Colors { lease, colors } => (lease, NativeCommand::Colors(colors)),
            Command::Scroll { lease, delta } => (lease, NativeCommand::Scroll(delta)),
            Command::Selection { lease, selection } => (lease, NativeCommand::Selection(selection)),
            Command::Copy { lease } => (lease, NativeCommand::Copy),
            Command::Hyperlink { lease, point } => (lease, NativeCommand::Hyperlink(point)),
            Command::MouseTracking { lease } => (lease, NativeCommand::MouseTracking),
            Command::RecoverAnsi { lease }
            | Command::Recover { lease, .. }
            | Command::Replay { lease, .. } => (lease, NativeCommand::Recover),
            _ => anyhow::bail!(
                "Runtime ownership and credit commands cannot be submitted by a terminal view"
            ),
        };
        ensure!(
            lease == expected,
            "Queued terminal intent belongs to a stale lease"
        );
        let action = NativeAction::Terminal {
            tab,
            attachment: attachment.clone(),
            command,
            input,
        };
        action.validate()?;
        Ok(action)
    }
}

pub(in crate::app) struct Connection {
    pub client: HostClient,
    pub tab: TabId,
    pub access: Access,
    sender: mpsc::Sender<Intent>,
    retirement: Option<oneshot::Sender<Retirement>>,
    frame_life: watch::Sender<bool>,
}
pub(in crate::app) struct Inbox {
    requests: mpsc::Receiver<Intent>,
    retirement: oneshot::Receiver<Retirement>,
}
impl Connection {
    pub fn new(client: HostClient, tab: TabId, access: Access) -> (Self, Inbox) {
        // One worker executes one intent at a time; one next intent may wait.
        let (sender, receiver) = mpsc::channel(1);
        let (retirement, retired) = oneshot::channel();
        let (frame_life, _) = watch::channel(false);
        (
            Self {
                client,
                tab,
                access,
                sender,
                retirement: Some(retirement),
                frame_life,
            },
            Inbox {
                requests: receiver,
                retirement: retired,
            },
        )
    }
    pub fn cancel_frames(&self) {
        self.frame_life.send_replace(true);
    }
    pub fn frame_cancellation(&self) -> watch::Receiver<bool> {
        self.frame_life.subscribe()
    }
    pub fn retire(&mut self, outbox: Outbox, next: u64) {
        self.cancel_frames();
        if let Some(sender) = self.retirement.take() {
            let _ = sender.send(Retirement::new(outbox, next));
        }
    }
    pub fn send(&self, intent: Intent) -> std::result::Result<(), SubmitError> {
        self.sender.try_send(intent).map_err(|error| match error {
            mpsc::error::TrySendError::Full(intent) => SubmitError::Full(intent.request),
            mpsc::error::TrySendError::Closed(intent) => SubmitError::Disconnected(intent.request),
        })
    }
}

pub(in crate::app) fn start_worker(
    view: &TerminalView,
    mut incoming: Inbox,
    cx: &mut Context<TerminalView>,
) {
    let Backend::Attached(connection) = &view.backend else {
        return;
    };
    let client = connection.client.clone();
    let access = connection.access.clone();
    let tab = connection.tab;
    cx.spawn(async move |view, cx| {
        let mut detach_attempted = false;
        while let Some(intent) = incoming.requests.recv().await {
            let request = intent.request;
            let input = intent.input;
            detach_attempted |=
                matches!(request.command, Command::RetryClose | Command::Close { .. });
            let result = match access.action_with_input(tab, request.command, input) {
                Ok(action) => execute(&client, action, cx).await,
                Err(error) => Err(error),
            };
            // Weak-view loss does not discard accepted presses/releases. First
            // drain the bounded transport, then the dropped view's outbox.
            let _ = view.update(cx, |view, cx| {
                let pending = view.pending.remove(&request.id.0);
                view.reply(pending, result, cx);
                view.flush_outbox();
                cx.notify();
            });
        }
        if let Ok(mut retirement) = incoming.retirement.await {
            while let Some(intent) = retirement.next_intent() {
                let request = intent.request;
                let input = intent.input;
                detach_attempted |=
                    matches!(request.command, Command::RetryClose | Command::Close { .. });
                if let Ok(action) = access.action_with_input(tab, request.command, input) {
                    let _ = execute(&client, action, cx).await;
                }
            }
        }
        // If a full control mailbox prevented enqueueing Close, detach only
        // after every retained release. Never replay an attempted mutation.
        if !detach_attempted {
            let _ = execute(&client, access.detach(tab), cx).await;
        }
    })
    .detach();
}

async fn execute(
    client: &HostClient,
    mut action: NativeAction,
    cx: &mut gpui::AsyncApp,
) -> Result<Response> {
    loop {
        let result = match action {
            NativeAction::Terminal {
                tab,
                attachment,
                command,
                input,
            } => client.command(tab, attachment, command, input).await,
            NativeAction::Detach { tab, attachment } => client
                .detach(tab, attachment)
                .await
                .map(|_| NativeResponse::Applied),
            action => client.execute(action).await,
        };
        match result {
            Ok(response) => return terminal_response(response),
            Err(error) => {
                action = retry(error)?;
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
            }
        }
    }
}

fn terminal_response(response: NativeResponse) -> Result<Response> {
    match response {
        // Includes Input receipts with zero-byte ignored encodings. Context and
        // activity policy are never reconstructed from the receipt in the UI.
        NativeResponse::Terminal(response) => Ok(response),
        NativeResponse::Applied => Ok(Response::Applied),
        _ => anyhow::bail!("Invalid native terminal response"),
    }
}

pub(super) fn retry(error: anyhow::Error) -> Result<NativeAction> {
    match error.downcast::<NativeAdmissionError>() {
        Ok(NativeAdmissionError::Full(action)) => Ok(*action),
        Ok(error) => Err(error.into()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
