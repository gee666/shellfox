use super::*;

impl Service {
    pub(crate) fn pump(&mut self) {
        use crate::owner::Step;
        let step = |service: &Self, step| service.owner_health.step(step);
        self.durability.log_lag();
        step(self, Step::TerminalEvents);
        let tabs: Vec<_> = self.terminals.keys().copied().collect();
        for tab in tabs {
            let _ = self.drain(tab);
        }
        step(self, Step::Closing);
        self.pump_closing();
        step(self, Step::Grants);
        let _ = self.pump_grant_retirements();
        step(self, Step::Catalog);
        self.pump_native_catalog();
        step(self, Step::Automation);
        self.pump_automation();
        step(self, Step::Activity);
        if self.stopping || self.metadata_closing {
            // Resource cleanup keeps every live touch, but SQL persistence waits
            // until cleanup/metadata-close has completed or safely failed.
            self.activity_collect_live();
            if self.ownership_lost.is_cancelled() {
                self.activity.publisher.invalidate();
            }
        } else {
            self.pump_activity();
            step(self, Step::Retention);
            self.pump_retention();
        }
        step(self, Step::Frames);
        self.native_capture_frames();
        step(self, Step::Watches);
        self.publish_native_watches();
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
        let mut woke = false;
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
            // Replies answer requests (frame reads included); waking on them would
            // release the next held frame read at once. Frame requests queue behind
            // this pass, so they see every event drained here.
            if !woke && !matches!(event, Event::Reply { .. }) {
                woke = true;
                self.native.watches.frames.wake(tab);
            }
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
                    if terminal.startup.is_none() {
                        terminal.startup = Some(result);
                    }
                    self.activity.dirty = true;
                }
                Event::Reply { id, result } => {
                    if let Ok(Response::Claimed(lease)) = &result {
                        terminal.control_epoch = lease.epoch;
                    }
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
