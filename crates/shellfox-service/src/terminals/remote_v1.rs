use super::*;
use shellfox_host::CapturedSettlement;
use shellfox_protocol::{AppError, ErrorCode, WriteTerminal};

#[derive(Clone, Copy, Default)]
pub(crate) struct RemoteV1Status {
    pub newest: Option<u64>,
    pub replay_lost: bool,
    pub exited: Option<u64>,
}

impl Service {
    pub(crate) fn remote_v1_input(
        &mut self,
        input: WriteTerminal,
        attachment: &Attachment,
    ) -> std::result::Result<(), AppError> {
        let settlement = input.settled_at.map_or(
            CapturedSettlement::Unavailable,
            CapturedSettlement::Captured,
        );
        self.input_with_context(
            input.tab_id,
            attachment,
            input.data.into_bytes(),
            input.user_initiated.unwrap_or(true),
            settlement,
        )
        .map_err(|_| AppError {
            code: ErrorCode::TargetLost,
            message: "Terminal input was not confirmed. Reattach before sending more.".into(),
            retryable: false,
        })
    }
    pub(crate) fn remote_v1_recover(
        &mut self,
        tab: TabId,
        attachment: &Attachment,
    ) -> Result<(Vec<u8>, u64, u16, u16)> {
        let lease = self.lease(tab, attachment)?;
        let Response::RemoteV1Ansi {
            bytes,
            last_sequence,
            cols,
            rows,
        } = self.call(
            tab,
            Command::Recover {
                lease,
                capability: shellfox_runtime::AnsiRecoveryCapability::RemoteV1,
            },
        )?
        else {
            bail!("Unexpected v1 recovery response.");
        };
        self.install_recovery_cut(tab, last_sequence)?;
        Ok((bytes, last_sequence, cols, rows))
    }
    /// What a v1 poll would see for this attachment, without consuming it.
    /// An error means the lease is gone, which the poll reports as Blocked.
    pub(crate) fn remote_v1_status(
        &self,
        tab: TabId,
        attachment: &Attachment,
    ) -> Result<RemoteV1Status> {
        self.lease(tab, attachment)?;
        let terminal = self
            .terminals
            .get(&tab)
            .context("Terminal is not running.")?;
        Ok(RemoteV1Status {
            newest: terminal.output.back().map(|chunk| chunk.sequence),
            replay_lost: terminal.replay_lost,
            exited: terminal.exited,
        })
    }
    pub(crate) fn remote_v1_output(
        &self,
        tab: TabId,
        attachment: &Attachment,
        after: u64,
    ) -> Result<(Vec<TerminalOutput>, bool, Option<u64>)> {
        self.lease(tab, attachment)?;
        let terminal = &self.terminals[&tab];
        Ok((
            terminal
                .output
                .iter()
                .filter(|chunk| chunk.sequence > after)
                .take(64)
                .cloned()
                .collect(),
            terminal.replay_lost,
            terminal.exited,
        ))
    }
}
