use super::*;

impl Service {
    pub(crate) fn pump(&mut self) {
        let _ = self.pump_remote_observer_retirements();
        let tabs: Vec<_> = self.terminals.keys().copied().collect();
        for tab in tabs {
            let _ = self.drain(tab);
        }
        let _ = self.pump_grant_retirements();
        self.pump_native_catalog();
        self.pump_automation();
        self.pump_ssh_native();
        if self.stopping || self.metadata_closing {
            // Resource cleanup keeps every live touch, but SQL persistence waits
            // until cleanup/metadata-close has completed or safely failed.
            self.activity_collect_live();
            if self.ownership_lost.is_cancelled() {
                self.activity.publisher.invalidate();
            }
        } else {
            self.pump_activity();
        }
        self.native_capture_frames();
    }
    pub(crate) fn drain(&mut self, tab: TabId) -> Result<()> {
        let identity = self
            .terminals
            .get(&tab)
            .context("Terminal is not running.")?
            .runtime
            .identity();
        // A viewer's mailbox never blocks replies/phase/cleanup. Reply activity
        // completion runs before a later Closed event can clear its contexts.
        for _ in 0..shellfox_runtime::EVENT_CAPACITY {
            let event = match self
                .terminals
                .get(&tab)
                .context("Terminal owner disappeared.")?
                .runtime
                .try_recv()
            {
                Ok(event) => event,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            };
            self.record_ownership_reply(identity, &event);
            self.native.record_event(tab, &event);
            let Some(event) = self.remote_v2_mailboxes.record(identity, event) else {
                continue;
            };
            let Some(event) = self.manager_v2_mailboxes.record(identity, event) else {
                continue;
            };
            if let Event::Reply { id, result } = &event {
                self.activity_input_reply(identity, *id, result);
            }
            if matches!(
                &event,
                Event::Phase {
                    phase: Phase::Closed,
                    ..
                }
            ) {
                self.cancel_activity_inputs(identity);
            }
            let terminal = self
                .terminals
                .get_mut(&tab)
                .context("Terminal owner disappeared.")?;
            match event {
                Event::Started(result) => {
                    let first = terminal.startup.is_none();
                    if first {
                        terminal.startup = Some(result);
                    }
                    if first {
                        self.activate_observer_factory_policy(tab);
                    }
                    self.activity.dirty = true;
                }
                Event::Reply { id, result } => {
                    if terminal.waiting.is_some_and(|waiting| waiting.0 == id.0) {
                        terminal.replies.clear();
                        terminal.replies.push_back((id, result));
                    }
                }
                Event::Output { lease, chunk } if terminal.lease == Some(lease) => {
                    if terminal.output_bytes + chunk.bytes.len() > MAX_PENDING_BYTES {
                        terminal.replay_lost = true;
                    } else {
                        terminal.output_bytes += chunk.bytes.len();
                        terminal.output.push_back(TerminalOutput {
                            sequence: chunk.sequence,
                            bytes: chunk.bytes,
                        });
                    }
                }
                Event::ReplayLost { lease, .. } if terminal.lease == Some(lease) => {
                    terminal.replay_lost = true
                }
                Event::Exited {
                    lease,
                    last_sequence,
                } if terminal.lease == Some(lease) => terminal.exited = Some(last_sequence),
                _ => {}
            }
        }
        Ok(())
    }
}
