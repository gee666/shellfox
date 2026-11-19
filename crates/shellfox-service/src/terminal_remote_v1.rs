use super::*;
use shellfox_host::CapturedSettlement;
use shellfox_protocol::{AppError, ErrorCode, WriteTerminal};

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
