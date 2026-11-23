use super::*;
use shellfox_ghostty_vt::TerminalColors;
use shellfox_protocol::OwnedAttachment;

impl Peer {
    pub(super) fn attach(
        &mut self,
        attachment: TerminalAttachment,
        colors: &TerminalColors,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.tab
                == match &attachment {
                    TerminalAttachment::Owned(owned) => owned.tab_id,
                    TerminalAttachment::Blocked { tab_id, .. } => *tab_id,
                },
            "Host attached the wrong terminal."
        );
        let cursor = ViewCursor::from_attachment(&attachment, self.connection)
            .map_err(|_| anyhow::anyhow!("Invalid terminal attachment."))?;
        if !matches!(self.state, State::Attaching(_)) {
            return Ok(());
        }
        let previous = std::mem::replace(&mut self.state, State::Offline);
        let State::Attaching(events) = previous else {
            return Ok(());
        };
        match attachment {
            TerminalAttachment::Blocked { generation, .. } => {
                self.attachment_generation = Some(generation);
                self.state = State::Blocked(generation);
            }
            TerminalAttachment::Owned(owned) => {
                self.attachment_generation = Some(owned.generation);
                let mut terminal = parser(&owned, colors)?;
                self.presentation = terminal.new_presentation_view()?;
                self.terminal = terminal;
                self.sequence = owned.last_sequence;
                let mut cursor = cursor;
                self.ack = Some(
                    cursor
                        .consumed(owned.last_sequence, self.connection)
                        .map_err(|_| anyhow::anyhow!("Invalid initial terminal sequence."))?,
                );
                self.size_to_send = self
                    .desired_size
                    .filter(|size| *size != (owned.cols, owned.rows));
                self.state = State::Attached {
                    cursor,
                    open: matches!(owned.state, ScreenState::Open),
                };
            }
        }
        for event in events {
            self.consume(event)?;
        }
        Ok(())
    }

    pub(super) fn consume(&mut self, event: TerminalEvent) -> anyhow::Result<bool> {
        let State::Attached { cursor, open } = &mut self.state else {
            // Electron can promote a waiting view when its former owner detaches.
            if let (
                State::Blocked(current),
                TerminalEvent::Control {
                    generation,
                    control: shellfox_protocol::Control::Owned,
                    ..
                },
            ) = (&self.state, event)
                && *current == generation
            {
                self.state = State::Offline;
            }
            return Ok(false);
        };
        let disposition = cursor
            .observe(&event, self.connection)
            .map_err(|_| anyhow::anyhow!("Invalid terminal output sequence."))?;
        match disposition {
            EventDisposition::Ignore => return Ok(false),
            EventDisposition::Reattach => {
                self.clear_operations();
                self.state = State::Offline;
                return Ok(false);
            }
            EventDisposition::Blocked => {
                if let TerminalEvent::Control { generation, .. } = event {
                    self.clear_operations();
                    self.state = State::Blocked(generation);
                }
                return Ok(false);
            }
            EventDisposition::Deliver => {}
        }
        match event {
            TerminalEvent::Data { sequence, data, .. } => {
                self.terminal.feed(data.as_bytes())?;
                self.sequence = sequence;
                let replies = self.terminal.drain_replies()?;
                self.ack =
                    Some(cursor.consumed(sequence, self.connection).map_err(|_| {
                        anyhow::anyhow!("Terminal output was not consumed in order.")
                    })?);
                // Rust's PTY owner answers queries itself. Electron's legacy
                // host delegates live query replies to the controlling view.
                if self.frontend_replies && !replies.is_empty() {
                    self.queue(replies, false)?;
                }
                Ok(true)
            }
            TerminalEvent::Exit { .. } => {
                *open = false;
                self.writes.clear();
                Ok(false)
            }
            TerminalEvent::Error { error, .. } => {
                self.operation_failed(error);
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    pub(super) fn operation_failed(&mut self, error: AppError) {
        let recover = matches!(self.state, State::Attached { .. })
            && (matches!(
                error.code,
                ErrorCode::TargetLost | ErrorCode::TerminalControlRequired
            ) || (error.code == ErrorCode::NativeUnavailable && error.retryable));
        if recover {
            self.clear_operations();
            self.state = State::Offline;
        } else {
            self.unavailable(error.message);
        }
    }

    pub(super) fn unavailable(&mut self, error: impl Into<String>) {
        self.clear_operations();
        self.state = State::Unavailable(error.into());
    }
}

fn parser(owned: &OwnedAttachment, colors: &TerminalColors) -> anyhow::Result<Terminal> {
    // Attach never asks for a suffix. Both a full replay and a truncated ANSI
    // recovery snapshot start with a fresh parser at the host's grid size.
    let mut terminal = Terminal::new(owned.cols, owned.rows)?;
    terminal.set_colors(colors)?;
    let mut next = owned.first_sequence;
    for chunk in &owned.chunks {
        anyhow::ensure!(
            chunk.sequence == next,
            "Terminal replay has a sequence gap."
        );
        terminal.feed(chunk.data.as_bytes())?;
        terminal.drain_replies()?;
        next += 1;
    }
    anyhow::ensure!(
        next == owned.last_sequence + 1,
        "Terminal replay has a sequence gap."
    );
    Ok(terminal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellfox_protocol::{DataTag, SessionId, TerminalData, TerminalLifetime};

    fn snapshot() -> OwnedAttachment {
        let tab_id = TabId::new();
        let generation = Generation::new();
        OwnedAttachment {
            tab_id,
            generation,
            session_id: SessionId::new(),
            control_epoch: 1,
            first_sequence: 41,
            last_sequence: 42,
            truncated: true,
            snapshot: Some(true),
            state: ScreenState::Open,
            exit_code: None,
            cols: 12,
            rows: 3,
            lifetime: TerminalLifetime::AppOwned,
            settled_at: None,
            chunks: ["\x1b[3;11", "HX!"]
                .into_iter()
                .enumerate()
                .map(|(i, data)| TerminalData {
                    kind: DataTag::Data,
                    tab_id,
                    generation,
                    sequence: 41 + i as u64,
                    data: data.into(),
                })
                .collect(),
        }
    }

    #[test]
    fn truncated_snapshot_uses_host_size_and_preserves_ansi_across_chunks() {
        let colors = crate::renderer::TerminalTheme::default().colors();
        let mut terminal = parser(&snapshot(), &colors).unwrap();
        let screen = terminal.render().unwrap();
        assert_eq!((screen.cols, screen.rows), (12, 3));
        assert_eq!(screen.cells[34].text, "X");
        assert_eq!(screen.cells[35].text, "!");
    }

    #[test]
    fn snapshot_with_missing_chunk_is_not_consumed() {
        let mut attachment = snapshot();
        attachment.chunks.remove(1);
        let colors = crate::renderer::TerminalTheme::default().colors();
        assert!(parser(&attachment, &colors).is_err());
    }
}
