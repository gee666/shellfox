use super::*;

impl Inner {
    pub(super) fn close(&mut self) -> Result<()> {
        self.output.discard();
        if let Some(input) = &self.input {
            input.close();
        }
        if self.assigned {
            self.job.terminate()?;
        } else if self.child.try_wait()?.is_none() {
            // No GO was sent, so this trusted process cannot have children.
            ensure!(
                unsafe { TerminateProcess(self.child.as_raw_handle().cast(), 1) } != 0
                    || self.child.try_wait()?.is_some(),
                "Unreleased raw supervisor termination failed."
            );
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(input) = &self.input {
                input.cancel();
            }
            if self.status.is_none() {
                self.status = self.child.try_wait()?.map(|s| s.code().unwrap_or(1));
            }
            let root_done = self.status.is_some();
            let empty = self.job.active_processes()? == 0;
            let input_done = match &mut self.input {
                Some(input) => input.join()?,
                None => true,
            };
            if root_done && empty && input_done && self.readers.iter().all(JoinHandle::is_finished)
            {
                for reader in self.readers.drain(..) {
                    reader
                        .join()
                        .map_err(|_| anyhow::anyhow!("Raw pipe worker panicked."))?;
                }
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "Raw process cleanup unconfirmed; original handles retained."
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for RawProcess {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            if let Err(mpsc::SendError(inner)) = self.reaper.send(inner) {
                std::mem::forget(inner);
            }
        }
    }
}

impl RawProcess {
    pub fn request_stop(&mut self) -> Result<()> {
        self.release_stdin();
        if let Some(inner) = &mut self.inner {
            if inner.status.is_none() {
                inner.status = inner.child.try_wait()?.map(|s| s.code().unwrap_or(1));
            }
            if inner.status.is_some() {
                inner.job.terminate()?;
                return Ok(());
            }
            let initiated = if inner.assigned {
                (unsafe { TerminateJobObject(inner.job.handle(), FORCED_EXIT_CODE) }) != 0
            } else {
                // Trusted unreleased startup owner, still pinned by its original handle.
                (unsafe { TerminateProcess(inner.child.as_raw_handle().cast(), FORCED_EXIT_CODE) })
                    != 0
            };
            ensure!(
                initiated || inner.child.try_wait()?.is_some(),
                "Owned raw termination was not initiated."
            );
            self.stop_initiated |= initiated;
        }
        Ok(())
    }
    /// User-facing forced-stop convention only. Cleanup still requires original
    /// root wait + empty Job + worker joins; natural exit races keep their code.
    pub fn was_forcibly_stopped(&self) -> bool {
        self.stop_initiated && self.exit_code() == Some(FORCED_EXIT_CODE as i32)
    }
}
