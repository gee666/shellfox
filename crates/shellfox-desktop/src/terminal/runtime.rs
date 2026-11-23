mod frames;
mod input;
mod input_context;
mod intent;
pub(in crate::terminal) mod native;
mod outbox;
pub(in crate::terminal) mod physical;
mod retire;
use super::{Backend, TerminalView, links};
use gpui::{ClipboardItem, Context};
pub(super) use outbox::Outbox;
use shellfox_runtime::{Command, ErrorCode, Response, SubmitError};

fn submission_error(error: SubmitError) -> String {
    match error {
        SubmitError::Invalid(error) => format!("Terminal request rejected: {error}"),
        SubmitError::Full(_) => "Terminal command queue is full".into(),
        SubmitError::Disconnected(_) => "Terminal owner disconnected".into(),
    }
}
fn stale_presentation(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<shellfox_runtime::RuntimeError>()
        .is_some_and(|error| error.code == ErrorCode::StalePresentation)
}
pub(super) enum Pending {
    Applied,
    SemanticSelection,
    Copy,
    Recovery,
    Close,
    Link { modifiers: gpui::Modifiers },
    LinkHover(links::HoverPoint),
}
#[derive(Default)]
pub(super) struct DeliveryState {
    consumed: u64,
    delivered: u64,
    pub(super) recovering: bool,
    recovery_barrier: Option<u64>,
    exited: Option<u64>,
    #[cfg(test)]
    acked: Option<u64>,
    #[cfg(test)]
    ack_pending: bool,
}
impl DeliveryState {
    pub fn finished(&self) -> bool {
        self.exited
            .is_some_and(|last| self.consumed >= last && self.delivered >= last)
    }
    #[cfg(test)]
    fn acknowledgement(&self) -> Option<u64> {
        let sequence = self.consumed.min(self.delivered);
        if self
            .recovery_barrier
            .is_some_and(|barrier| sequence < barrier)
        {
            return None;
        }
        (!self.ack_pending && self.acked.is_none_or(|acked| sequence > acked)).then_some(sequence)
    }
}
impl TerminalView {
    pub(super) fn submit(&mut self, command: Command, pending: Pending) -> bool {
        if !matches!(self.backend, Backend::Attached(_)) || self.detached {
            return false;
        }
        if self.control_blocked && !matches!(command, Command::RetryClose | Command::Close { .. }) {
            self.reject("Terminal control belongs to another view");
            return false;
        }
        let user_input = matches!(
            &command,
            Command::Paste { .. }
                | Command::Key {
                    event: shellfox_ghostty_vt::key::KeyEvent {
                        action: shellfox_ghostty_vt::key::KeyAction::Press,
                        ..
                    },
                    ..
                }
                | Command::Mouse {
                    event: shellfox_ghostty_vt::MouseEvent {
                        action: shellfox_ghostty_vt::MouseAction::Press,
                        ..
                    },
                    ..
                }
        );
        if user_input
            && self
                .phase
                .is_some_and(|phase| phase != shellfox_runtime::Phase::Active)
        {
            self.reject("Terminal is not active; input was not accepted");
            return false;
        }
        let input = if input_context::input_command(&command) {
            match self.frozen_input(&command) {
                Some(input) => Some(input),
                None => {
                    self.reject("Session snapshot is not ready; input was not accepted");
                    return false;
                }
            }
        } else {
            None
        };
        if let Err(error) = self.outbox.enqueue_with_input(command, pending, input) {
            self.reject(error);
            return false;
        }
        if user_input {
            self.notice = None;
        }
        self.flush_outbox()
    }
    pub(super) fn flush_outbox(&mut self) -> bool {
        let Backend::Attached(connection) = &self.backend else {
            return false;
        };
        if let Some(error) = self.outbox.flush_with_input(
            &mut self.next_request,
            &mut self.pending,
            |request, input| {
                connection.send(intent::Intent {
                    request,
                    input: input.cloned(),
                })
            },
        ) {
            self.close_pending = false;
            self.failed = true;
            self.reject(submission_error(error));
            return false;
        }
        true
    }
    fn reply(
        &mut self,
        pending: Option<Pending>,
        result: anyhow::Result<Response>,
        cx: &mut Context<Self>,
    ) {
        match (pending, result) {
            (Some(Pending::Recovery), Ok(_)) => {
                self.delivery.recovering = false;
            }
            (Some(Pending::Applied), Ok(Response::Input(_))) => {
                // The service already applied the original input context/activity policy.
                // A receipt is success even when the encoder intentionally wrote zero bytes.
            }
            (Some(Pending::Close), Ok(Response::Applied)) => {
                self.detached = true;
                self.close_pending = false;
                self.outbox.stop_input(true);
                self.status = "Detached terminal view".into();
            }
            (Some(Pending::Copy), Ok(Response::Text(Some(text)))) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text))
            }
            (Some(Pending::Link { modifiers }), Ok(Response::Text(text))) => {
                self.open_link(text, modifiers, cx);
            }
            (Some(Pending::LinkHover(at)), result) => {
                let uri = match result { Ok(Response::Text(text)) => text, _ => None };
                self.apply_link_hover(at, uri, cx);
            }
            (pending, Err(error)) => {
                if matches!(pending, Some(Pending::Close)) {
                    self.close_pending = false;
                }
                if matches!(pending, Some(Pending::Recovery)) {
                    self.delivery.recovering = false;
                }
                if stale_presentation(&error) {
                    // Drop this stale intent. A fresh native pull/publication is
                    // scheduled by the existing poll; never retry old coordinates.
                    self.semantic_binding = None;
                    self.notice =
                        Some("Selection view changed; requesting a fresh accessibility frame".into());
                    cx.notify();
                } else if control_required(&error) {
                    self.block_control();
                } else if error
                    .downcast_ref::<shellfox_runtime::RuntimeError>()
                    .is_some_and(|error| error.code == ErrorCode::ReplayLost)
                {
                    self.recover();
                } else {
                    self.reject(format!("Terminal request rejected: {error}"));
                }
            }
            _ => {}
        }
    }
    fn block_control(&mut self) {
        self.control_blocked = true;
        self.status = "Terminal controlled by another view".into();
        self.notice = Some(self.status.clone());
    }
    fn recover(&mut self) {
        if self.failed || self.closing || self.delivery.recovering {
            return;
        }
        let command = match &self.backend {
            Backend::Attached(connection) => match &connection.access {
                native::Access::Control(attachment) => Command::RecoverAnsi {
                    lease: shellfox_runtime::Lease {
                        controller: attachment.controller,
                        epoch: attachment.epoch,
                    },
                },
                native::Access::Observer(attachment) => Command::ObserverRecover {
                    observer: shellfox_runtime::ObserverLease {
                        viewer: attachment.viewer,
                        epoch: attachment.epoch,
                    },
                    capability: shellfox_runtime::AnsiRecoveryCapability::VisibleScreen,
                },
            },
            Backend::Fixture(_) | Backend::Peer(_) => return,
        };
        self.delivery.recovering = self.submit(command, Pending::Recovery);
    }
    pub(super) fn sync_configuration(&mut self) {
        if self.failed || self.closing {
            return;
        }
        if let Some(lease) = self.lease {
            self.submit(
                Command::Colors {
                    lease,
                    colors: Box::new(self.theme.colors()),
                },
                Pending::Applied,
            );
            self.submit(
                Command::Resize {
                    lease,
                    cols: self.grid.0,
                    rows: self.grid.1,
                },
                Pending::Applied,
            );
        }
    }
}
#[cfg(test)]
mod tests;

/// Another client controls the terminal; this one may only watch until it takes control.
pub(crate) fn control_required(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<shellfox_runtime::RuntimeError>()
        .is_some_and(|error| error.code == ErrorCode::ControlRequired)
}
