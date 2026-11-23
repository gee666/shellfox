use super::*;

impl AutomationRuns {
    /// Reserve the request and its one byte source before launching the target.
    /// Some is an idempotent completed Start. On None, take_source once and start
    /// Runtime::start_external before calling start with the SAME request/owner.
    pub fn reserve_start(
        &mut self,
        input: &AutomationStart,
        owner: u64,
    ) -> Result<Option<AutomationRun>> {
        input.validate()?;
        ensure!(owner > 0, "Invalid automation controller.");
        ensure!(!self.closing, "The host is shutting down.");
        self.pump()?;
        ensure!(
            !self.retired.contains_key(&input.request_id),
            "Run request ID has an unacknowledged parser retirement."
        );
        if let Some(run) = self.runs.get(&input.request_id) {
            ensure!(
                run.owner == owner && run.input == *input,
                "Run request ID already used."
            );
            return Ok(Some(run.dto.clone()));
        }
        if let Some(prepared) = self.prepared.get(&input.request_id) {
            ensure!(
                prepared.owner == owner && prepared.input == *input,
                "Run request ID already used."
            );
            return Ok(None);
        }
        ensure!(
            self.failed.is_empty(),
            "A failed automation launch still owns cleanup resources."
        );
        RawCommand {
            executable: input.executable.clone(),
            args: input.args.clone(),
            cwd: input.cwd.clone(),
            env: input.env.clone(),
        }
        .validate()?;
        if self.runs.len() + self.prepared.len() >= AUTOMATION_RUNS {
            let retired = self
                .runs
                .iter()
                .find(|(_, r)| r.dto.state == AutomationState::Exited && r.frames.is_empty())
                .map(|(id, _)| *id);
            if let Some(id) = retired {
                self.retire_for_admission(id)?;
            } else {
                anyhow::bail!("The host has 64 retained automation runs.");
            }
        }
        let (source, publisher) = AutomationView::new();
        self.prepared.insert(
            input.request_id,
            Prepared {
                input: input.clone(),
                owner,
                source: Some(source),
                source_identity: None,
                publisher,
            },
        );
        Ok(None)
    }
    /// Bind the canonical ready Runtime before raw launch. Immutable generation
    /// identity is later carried by retirement, not inferred from a reusable ID.
    pub fn bind_source_identity(
        &mut self,
        id: Uuid,
        identity: shellfox_runtime::Identity,
    ) -> Result<()> {
        let prepared = self
            .prepared
            .get_mut(&id)
            .context("Automation source reservation not found.")?;
        ensure!(
            prepared.source.is_none(),
            "Take the automation source before binding its Runtime identity."
        );
        ensure!(
            prepared.source_identity.is_none_or(|old| old == identity),
            "Automation Runtime identity is immutable."
        );
        prepared.source_identity = Some(identity);
        Ok(())
    }
    /// Cancel only a reservation whose raw target has never been launched.
    pub fn cancel_reserved(&mut self, id: Uuid, owner: u64) -> Result<()> {
        let prepared = self
            .prepared
            .get(&id)
            .context("Automation reservation not found.")?;
        ensure!(
            prepared.owner == owner,
            "Only the starting client can cancel this reservation."
        );
        self.prepared.remove(&id);
        Ok(())
    }
    pub fn cleanup_pending(&self, id: Uuid) -> bool {
        self.failed.iter().any(|failed| failed.id == id)
    }
    pub fn start(&mut self, input: AutomationStart, owner: u64) -> Result<AutomationRun> {
        self.start_inner(input, owner, None)
    }
    pub(super) fn start_inner(
        &mut self,
        input: AutomationStart,
        owner: u64,
        launch_binding: Option<shellfox_terminal::NativeLaunchBinding>,
    ) -> Result<AutomationRun> {
        if let Some(existing) = self.reserve_start(&input, owner)? {
            return Ok(existing);
        }
        let prepared = self
            .prepared
            .remove(&input.request_id)
            .context("Automation source reservation disappeared.")?;
        // In the shared Service path source is already owned by its ready Runtime.
        // Direct fixture consumers may take this same source immediately after
        // spawn. OS/read-queue credit preserves all bytes until they do so.
        let source = prepared.source;
        let source_identity = prepared.source_identity;
        let publisher = prepared.publisher;
        let command = RawCommand {
            executable: input.executable.clone(),
            args: input.args.clone(),
            cwd: input.cwd.clone(),
            env: input.env.clone(),
        };
        let spawned = match launch_binding {
            Some(binding) => {
                RawProcess::spawn_with_launch_binding(&self.supervisor, command, binding)
            }
            None => RawProcess::spawn(&self.supervisor, command),
        };
        let mut process = match spawned {
            Ok(process) => process,
            Err(error) => {
                if let Some(process) = error.take_process() {
                    self.failed.push(FailedProcess {
                        id: input.request_id,
                        process,
                        _publisher: publisher,
                    });
                }
                return Err(error.into());
            }
        };
        let title = input.title.clone().unwrap_or_else(|| {
            let name = std::path::Path::new(&input.executable)
                .file_name()
                .and_then(|p| p.to_str())
                .unwrap_or(&input.executable);
            let mut units = 0;
            name.chars()
                .take_while(|c| {
                    units += c.len_utf16();
                    units <= 200
                })
                .collect()
        });
        let dto = AutomationRun {
            id: input.request_id,
            pid: process.pid(),
            executable: input.executable.clone(),
            cwd: input.cwd.clone(),
            title,
            state: AutomationState::Running,
            exit_code: None,
            signal: None,
            session_id: None,
            tab_id: None,
        };
        // Without a wake (or if its exit watch can't start) the owner polls it.
        let polled = match &self.wake {
            Some(wake) => process.set_output_wake(wake.clone()).is_err(),
            None => true,
        };
        let launch_evidence = process.take_launch_evidence();
        let detached = input.detached;
        if detached {
            process.release_stdin();
        }
        self.runs.insert(
            dto.id,
            Run {
                input,
                owner,
                process: Some(process),
                dto: dto.clone(),
                frames: VecDeque::new(),
                bytes: 0,
                sequence: 0,
                acknowledged: 0,
                capturing: !detached,
                last_read: Instant::now(),
                input_ended: detached,
                stopping: false,
                eof: [false; 2],
                outcome: None,
                forced_stop: false,
                source,
                source_identity,
                publisher,
                pending: None,
                source_failure: None,
                failed_cleanup: None,
                launch_evidence,
                polled,
                changes: self.changes.clone(),
            },
        );
        Ok(dto)
    }
}
