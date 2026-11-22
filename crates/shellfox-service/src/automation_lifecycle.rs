use super::*;

impl AutomationRuns {
    /// Always runs on the shared Service owner, including before the first Show.
    /// A Full authoritative feed retains its exact event and backpressures pipes.
    pub fn pump(&mut self) -> Result<()> {
        self.failed
            .retain_mut(|failed| failed.process.close().is_err());
        for run in self.runs.values_mut() {
            if run.capturing && run.last_read.elapsed() > Duration::from_secs(30) {
                run.release();
            }
            if run.source_failure.is_some() {
                run.poll_failed_cleanup()?;
                continue;
            }
            if run.dto.state == AutomationState::Exited {
                continue;
            }
            ensure!(
                run.publisher.alive(),
                "Automation authoritative screen stopped; raw owner retained for cleanup."
            );
            if let Some(process) = &mut run.process {
                if let Some(code) = process.poll_exit()? {
                    run.outcome = Some(code);
                    run.forced_stop = process.was_forcibly_stopped();
                }
            }
            for _ in 0..32 {
                if !run.accept_pending()? {
                    break;
                }
                if run.capturing && run.bytes >= AUTOMATION_CREDIT {
                    break;
                }
                let Some(output) = run.process.as_ref().and_then(RawProcess::try_output) else {
                    break;
                };
                let stream = |stream| {
                    if stream == RawStream::Stdout {
                        AutomationStream::Stdout
                    } else {
                        AutomationStream::Stderr
                    }
                };
                run.pending = Some(match output {
                    RawOutput::Data {
                        stream: descriptor,
                        bytes,
                    } => {
                        let sequence = run
                            .sequence
                            .checked_add(1)
                            .filter(|s| *s <= shellfox_protocol::MAX_SAFE_INTEGER)
                            .context("Automation source sequence exhausted.")?;
                        AutomationViewEvent::Data {
                            sequence,
                            stream: stream(descriptor),
                            bytes,
                        }
                    }
                    RawOutput::Eof(descriptor) => AutomationViewEvent::Eof {
                        after_sequence: run.sequence,
                        stream: stream(descriptor),
                    },
                    RawOutput::Failed(_) => anyhow::bail!(
                        "Automation output pipe failed; process owner retained for cleanup."
                    ),
                });
            }
            if !run.accept_pending()? {
                continue;
            }
            if run.eof == [true; 2] {
                if let Some(code) = run.outcome {
                    if let Some(process) = &mut run.process {
                        process.close()?;
                    }
                    run.process = None;
                    run.pending = Some(AutomationViewEvent::Exit {
                        after_sequence: run.sequence,
                        code,
                    });
                    run.accept_pending()?;
                }
            }
        }
        Ok(())
    }
    /// Disconnect closes stdin and releases raw collection, never process ownership
    /// or the one authoritative screen Runtime. That screen still receives bytes.
    pub fn release_owner(&mut self, owner: u64) {
        for run in self.runs.values_mut().filter(|run| run.owner == owner) {
            run.release();
        }
    }
    pub fn begin_stop(&mut self, request: AutomationStop) -> Result<()> {
        if self.retired.contains_key(&request.id) {
            return Ok(());
        }
        let run = self
            .runs
            .get_mut(&request.id)
            .context("Automation run not found.")?;
        run.stopping = true;
        // A different local client can stop a run whose original reader has a
        // full credit window. End that pipe lease, never its authoritative source.
        run.release();
        if run.dto.state == AutomationState::Exited && run.process.is_none() {
            return Ok(());
        }
        if !run.publisher.alive() {
            // Failed source teardown is distinct from ordered EOF/Finish.
            if run.source_failure.is_none() {
                run.source_failure = Some(run.dropped_source_failure());
            }
            if let Some(process) = &mut run.process {
                process.request_stop()?;
            }
            run.poll_failed_cleanup()?;
        } else if let Some(process) = &mut run.process {
            process.request_stop()?;
        }
        Ok(())
    }
    pub fn is_stopped(&self, id: Uuid) -> Result<bool> {
        if self.retired.contains_key(&id) {
            return Ok(true);
        }
        let run = self.runs.get(&id).context("Automation run not found.")?;
        Ok(run.process.is_none() && run.dto.state == AutomationState::Exited)
    }
    pub(crate) fn completed_view_exit_code(
        &self,
        id: Uuid,
        identity: shellfox_runtime::Identity,
    ) -> Option<i64> {
        let run = self.runs.get(&id)?;
        if run.source_identity != Some(identity)
            || run.dto.session_id.is_none()
            || run.dto.tab_id.is_none()
            || run.dto.state != AutomationState::Exited
            || run.process.is_some()
        {
            return None;
        }
        run.outcome.map(compatible_natural_exit)
    }

    /// Test convenience for independent source consumers. The shared Service
    /// uses begin_stop/is_stopped while pumping its source relay.
    #[cfg(test)]
    pub fn stop(&mut self, request: AutomationStop) -> Result<AutomationStopped> {
        self.begin_stop(AutomationStop { id: request.id })?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self.is_stopped(request.id)? {
                return Ok(AutomationStopped { stopped: true });
            }
            self.pump()?;
            ensure!(
                Instant::now() < deadline,
                "Automation stop is unconfirmed; raw owner and pending feed retained."
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    /// Nonblocking normal shutdown path. Service keeps its source relays and
    /// external parsers alive, pumps them and this manager until shutdown_ready.
    pub fn begin_shutdown(&mut self) -> Result<()> {
        self.closing = true;
        let mut first_error = None;
        self.prepared.clear();
        self.failed
            .retain_mut(|failed| match failed.process.close() {
                Ok(()) => false,
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    true
                }
            });
        for run in self.runs.values_mut() {
            run.release();
            // Startup failed before its one parser could be installed.
            run.source = None;
        }
        let ids: Vec<_> = self.runs.keys().copied().collect();
        for id in &ids {
            if let Err(error) = self.begin_stop(AutomationStop { id: *id }) {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }
    /// Raw owners are confirmed gone and final Exit events are queued. Service
    /// must ALSO finish each relay and its Runtime after all source data/EOFs.
    pub fn shutdown_ready(&self) -> bool {
        self.closing
            && self.failed.is_empty()
            && self.prepared.is_empty()
            && self
                .runs
                .values()
                .all(|run| run.process.is_none() && run.dto.state == AutomationState::Exited)
    }
    /// Test convenience when source consumers run independently of this thread.
    #[cfg(test)]
    pub fn shutdown(&mut self) -> Result<()> {
        self.begin_shutdown()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.shutdown_ready() {
            self.pump()?;
            ensure!(
                Instant::now() < deadline,
                "Automation shutdown is unconfirmed; raw and source owners retained."
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }
    pub fn active_processes(&self) -> usize {
        self.failed.len()
            + self
                .runs
                .values()
                .filter(|run| run.process.is_some())
                .count()
    }
}
