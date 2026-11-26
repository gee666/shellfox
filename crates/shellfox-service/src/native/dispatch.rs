use super::{
    cache::{Grant, View, ViewKey},
    *,
};
use anyhow::{ensure, Context, Result};

pub(super) fn control_key(controller: u64, tab: TabId) -> ViewKey {
    ViewKey {
        controller,
        tab,
        observer: false,
    }
}
impl crate::Service {
    pub fn execute_native(
        &mut self,
        action: NativeAction,
        controller: u64,
    ) -> Result<NativeResponse> {
        action.validate()?;
        self.assert_owned()?;
        if self.stopping
            && !matches!(
                action,
                NativeAction::Status
                    | NativeAction::ChangedSince { .. }
                    | NativeAction::Frame { .. }
                    | NativeAction::ObserverFrame { .. }
                    | NativeAction::RecoverObserver { .. }
                    | NativeAction::AcceptFrame { .. }
                    | NativeAction::AcceptObserverFrame { .. }
            )
        {
            self.assert_accepting()?;
        }
        ensure!(controller != 0, "A native controller identity is required.");
        let response = match action {
            NativeAction::Power(_) => {
                anyhow::bail!("Keep-awake requests require the owned blocking worker route.")
            }
            NativeAction::TerminalProfiles => {
                anyhow::bail!("Native catalog discovery requires its protected owner worker route.")
            }
            NativeAction::TabStats => {
                anyhow::bail!("Tab stats are captured on their own worker route.")
            }
            NativeAction::SessionRecords => NativeResponse::SessionRecords(self.session_records()?),
            NativeAction::Status => NativeResponse::Status(NativeStatus {
                active_terminals: self.active_terminals(),
                pid: std::process::id(),
                started_at: self.started_at.clone(),
                desktop: self
                    .system_native_handler
                    .as_ref()
                    .is_some_and(|system| system.desktop_actions()),
            }),
            NativeAction::ReadWindowState => {
                NativeResponse::WindowState(self.host().window_state()?)
            }
            NativeAction::ReadTabActivity => {
                NativeResponse::TabActivity(self.desktop_tab_activity())
            }
            NativeAction::SetVisibleSessions(report) => {
                self.activity
                    .report_desktop_visibility(controller, report.session_ids);
                NativeResponse::Applied
            }
            NativeAction::SaveWindowState(state) => {
                self.host().save_window_state(state)?;
                NativeResponse::Applied
            }
            NativeAction::Metadata(request) => {
                NativeResponse::Snapshot(self.application_metadata(request)?)
            }
            NativeAction::CreationRequestReceipt { request_id } => {
                NativeResponse::CreationRequestReceipt {
                    request_id,
                    receipt: self.host().creation_request_receipt(request_id)?,
                }
            }
            NativeAction::Ssh(_) | NativeAction::System(_) => {
                anyhow::bail!("Device file actions run on the device file worker.")
            }
            NativeAction::Remote(_) => {
                anyhow::bail!("Remote controls require the owned async manager route.")
            }
            NativeAction::Settings(request) => {
                NativeResponse::Settings(self.host().settings_request(request)?)
            }
            NativeAction::SelectTab { tab } => {
                NativeResponse::Snapshot(self.host().select_tab(tab)?)
            }
            NativeAction::SessionCwd { session, cwd } => {
                NativeResponse::Snapshot(self.host().set_session_cwd(session, &cwd)?)
            }
            NativeAction::TabCwd { tab, cwd } => {
                NativeResponse::Snapshot(self.host().set_tab_cwd(tab, &cwd)?)
            }
            NativeAction::SetSettled { session, settled } => {
                NativeResponse::Snapshot(self.host().set_session_settled(session, settled)?)
            }
            NativeAction::ClearSessionError { session } => {
                NativeResponse::Snapshot(self.acknowledge_session_error(session)?)
            }
            NativeAction::ChangedSince { revision } => {
                NativeResponse::Changed(self.host().changed_since(revision)?)
            }
            NativeAction::CloseTab { .. }
            | NativeAction::CloseTerminal(_)
            | NativeAction::DeleteSession { .. } => {
                anyhow::bail!("Closes run on the owner's close route.")
            }
            NativeAction::Attach { tab, takeover } => {
                self.native_attach(tab, controller, takeover)?
            }
            NativeAction::Detach { tab, attachment } => {
                ensure!(
                    attachment.controller == controller,
                    "Native attachment belongs to another client."
                );
                let key = control_key(controller, tab);
                let same_view = self.native.views.get(&key).is_some_and(|view|
                    matches!(&view.grant, Grant::Control(current) if current.epoch == attachment.epoch
                        && current.generation == attachment.generation));
                if same_view {
                    // A takeover may already have revoked this view. Detaching
                    // an old token must never detach the new controller/view.
                    if self.lease(tab, &attachment).is_ok() {
                        self.detach_attachment(tab, &attachment)?;
                    }
                    self.native.remove(key);
                }
                NativeResponse::Applied
            }
            NativeAction::Terminal {
                tab,
                attachment,
                command,
                input,
            } => {
                self.native_control(tab, controller, &attachment)?;
                let response = self.native_command(tab, controller, &attachment, command, input)?;
                self.native.watches.frames.wake(tab);
                response
            }
            NativeAction::Frame { tab, attachment } => self.native_presentation(
                tab,
                controller,
                NativeViewAttachment::Controller(attachment),
                NativePresentation::Frame { focused: true },
            )?,
            NativeAction::Presentation {
                tab,
                attachment,
                action,
            } => {
                let frame = matches!(action, NativePresentation::Frame { .. });
                let response = self.native_presentation(tab, controller, attachment, action)?;
                if !frame {
                    // Scroll, selection and resize change the next frame.
                    self.native.watches.frames.wake(tab);
                }
                response
            }
            NativeAction::AcceptFrame {
                tab,
                attachment,
                frame_id,
            } => {
                self.native_control(tab, controller, &attachment)?;
                let key = control_key(controller, tab);
                let sequence = self
                    .native
                    .views
                    .get_mut(&key)
                    .context("Native view is detached.")?
                    .accepted_sequence(frame_id)?;
                let acked = self.native.views[&key].acked;
                if acked.is_none_or(|last| sequence > last) {
                    // The service validates the same generation/lease and runtime
                    // highest-delivered/barrier before credit can advance.
                    self.acknowledge(tab, &attachment, sequence)?;
                    let view = self
                        .native
                        .views
                        .get_mut(&key)
                        .context("Native view disappeared.")?;
                    view.acked = Some(sequence);
                    view.barrier = None;
                }
                NativeResponse::Accepted { sequence }
            }
            NativeAction::Observe { tab } => self.native_observe(tab, controller)?,
            NativeAction::ObserverFrame { tab, attachment } => {
                self.native_observer_frame(tab, controller, &attachment)?
            }
            NativeAction::RecoverObserver { tab, attachment } => {
                self.native_recover_observer(tab, controller, &attachment)?
            }
            NativeAction::AcceptObserverFrame {
                tab,
                attachment,
                frame_id,
            } => self.native_accept_observer(tab, controller, &attachment, frame_id)?,
            NativeAction::Unobserve { tab, attachment } => {
                self.native_unobserve(tab, controller, &attachment)?;
                NativeResponse::Applied
            }
            NativeAction::DetachAll => {
                self.native_detach_all(controller)?;
                NativeResponse::Applied
            }
        };
        Ok(response)
    }

    pub(super) fn native_control(
        &self,
        tab: TabId,
        controller: u64,
        attachment: &Attachment,
    ) -> Result<()> {
        let denied = || shellfox_runtime::RuntimeError {
            code: shellfox_runtime::ErrorCode::ControlRequired,
            message: "Terminal control is detached, stale, or owned by another view.".into(),
        };
        if attachment.controller != controller {
            return Err(denied().into());
        }
        self.lease(tab, attachment).map_err(|_| denied())?;
        let view = self
            .native
            .views
            .get(&control_key(controller, tab))
            .ok_or_else(denied)?;
        if !matches!(&view.grant, Grant::Control(current) if current.epoch == attachment.epoch
            && current.generation == attachment.generation)
        {
            return Err(denied().into());
        }
        Ok(())
    }

    fn native_attach(
        &mut self,
        tab: TabId,
        controller: u64,
        takeover: bool,
    ) -> Result<NativeResponse> {
        let key = control_key(controller, tab);
        self.native.admit(key)?;
        let attachment = self.attach(tab, controller, takeover)?;
        // The native view accepts an immutable screen of the SAME parser, not
        // ANSI replay into another parser. Recovery establishes its credit fence.
        let (_, barrier, _) = self.recover(tab, &attachment)?;
        self.native
            .views
            .retain(|key, _| key.tab != tab || key.observer);
        self.native.views.insert(
            key,
            View::new(
                Grant::Control(attachment.clone()),
                barrier,
                self.terminals[&tab].runtime.phase(),
            ),
        );
        Ok(NativeResponse::Attached(attachment))
    }
}
