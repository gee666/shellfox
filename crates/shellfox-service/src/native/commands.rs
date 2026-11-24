use super::{cache::View, dispatch::control_key, *};
use anyhow::{Context, Result};
use shellfox_runtime::Command;

impl crate::Service {
    pub(super) fn native_command(
        &mut self,
        tab: TabId,
        controller: u64,
        attachment: &Attachment,
        command: NativeCommand,
        input: Option<NativeInputContext>,
    ) -> Result<NativeResponse> {
        let lease = self.lease(tab, attachment)?;
        let command = match command {
            NativeCommand::Key(event) => Command::Key { lease, event },
            NativeCommand::Keys(events) => Command::Keys { lease, events },
            NativeCommand::Paste(text) => Command::Paste { lease, text },
            NativeCommand::Mouse { event, geometry } => Command::Mouse {
                lease,
                event,
                geometry,
            },
            NativeCommand::Resize { cols, rows } => Command::Resize { lease, cols, rows },
            NativeCommand::Colors(colors) => Command::Colors { lease, colors },
            NativeCommand::Scroll(delta) => {
                return self.native_presentation(
                    tab,
                    controller,
                    NativeViewAttachment::Controller(attachment.clone()),
                    NativePresentation::Scroll { delta },
                )
            }
            NativeCommand::Selection(selection) => {
                return self.native_presentation(
                    tab,
                    controller,
                    NativeViewAttachment::Controller(attachment.clone()),
                    NativePresentation::Selection { selection },
                )
            }
            NativeCommand::Copy => {
                return self.native_presentation(
                    tab,
                    controller,
                    NativeViewAttachment::Controller(attachment.clone()),
                    NativePresentation::Copy,
                )
            }
            NativeCommand::Hyperlink(point) => {
                return self.native_presentation(
                    tab,
                    controller,
                    NativeViewAttachment::Controller(attachment.clone()),
                    NativePresentation::Hyperlink { point },
                )
            }
            NativeCommand::MouseTracking => Command::MouseTracking { lease },
            NativeCommand::Recover => {
                let (_, barrier, _) = self.recover(tab, attachment)?;
                let key = control_key(controller, tab);
                let grant = self
                    .native
                    .views
                    .get(&key)
                    .context("Native view detached.")?
                    .grant
                    .clone();
                self.native.views.insert(
                    key,
                    View::new(grant, barrier, self.terminals[&tab].runtime.phase()),
                );
                // No ANSI is sent to a second parser. The following immutable
                // native frame is the authoritative recovery for this view.
                return Ok(NativeResponse::Applied);
            }
        };
        let response = if let Some(context) = input {
            let admission = crate::activity::integration::InputAdmission::native(
                context.user_initiated,
                context.hint,
                context.captured_settlement,
            );
            self.call_input(tab, command, admission)?
        } else {
            self.call(tab, command)?
        };
        Ok(NativeResponse::Terminal(response))
    }
}
