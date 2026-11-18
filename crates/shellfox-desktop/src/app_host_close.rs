use super::*;
use std::io::Write as _;

impl<B: AppHostBackend> AppHost<B> {
    /// Reserve targets AFTER approval, before draining their UI input outboxes.
    /// This freezes registry membership while Roots await their reserved key and
    /// mouse releases. It does not detach a client or request Stop.
    pub async fn begin_close(&self, ticket: &CloseTicket) -> Result<Vec<WindowId>> {
        let _operation = self.shared.operation.lock().await;
        self.reserve_close(ticket)?;
        Ok(self
            .state()?
            .pending
            .as_ref()
            .context("Missing close operation.")?
            .windows
            .clone())
    }
    /// A UI drain failure may cancel a reservation, but never an accepted Stop.
    pub fn abort_close(&self, ticket: &CloseTicket) -> Result<()> {
        let mut state = self.state()?;
        let pending = state.pending.as_ref().context("Missing close operation.")?;
        ensure!(
            ticket.owner.ptr_eq(&Arc::downgrade(&self.shared.identity))
                && pending.ticket.revision == ticket.revision
                && pending.ticket.target == ticket.target
                && !pending.finishing,
            "A finishing close cannot be aborted."
        );
        state.pending = None;
        state.revision += 1;
        Ok(())
    }
    fn reserve_close(&self, ticket: &CloseTicket) -> Result<()> {
        {
            let mut state = self.state()?;
            ensure!(
                ticket.owner.ptr_eq(&Arc::downgrade(&self.shared.identity)),
                "Close ticket belongs to another application host."
            );
            if let Some(pending) = &state.pending {
                ensure!(
                    pending.ticket.revision == ticket.revision
                        && pending.ticket.target == ticket.target,
                    "Another close operation is pending."
                );
            } else {
                ensure!(
                    !state.stopped && state.revision == ticket.revision,
                    "Window state changed; recheck guards and quit confirmation."
                );
                let windows = Self::targets(&state, ticket.target)?;
                ensure!(
                    windows
                        .iter()
                        .all(|id| state.windows[id].guard == WindowCloseGuard::Ready),
                    "Save/discard files and await operations before closing."
                );
                let stop_host = windows.len() == state.windows.len();
                state.pending = Some(PendingClose {
                    ticket: ticket.clone(),
                    windows,
                    detached: HashSet::new(),
                    stop_host,
                    finishing: false,
                });
            }
        }
        Ok(())
    }
    /// Keep the Global alive until this succeeds. UI outboxes must be drained
    /// first. A cancelled wait resumes the same ticket; a returned failure
    /// invalidates approval and permits retry. Never remove windows/quit on Err.
    pub async fn finish_close(&self, ticket: CloseTicket) -> Result<ClosedWindows> {
        let _operation = self.shared.operation.lock().await;
        self.reserve_close(&ticket)?;
        self.state()?
            .pending
            .as_mut()
            .context("Missing close operation.")?
            .finishing = true;
        let result = self.complete_pending().await;
        if result.is_err() {
            let mut state = self.state()?;
            // The backend may already have accepted Stop. It retains its own
            // receiver/owner. New windows cannot revive a partly stopped host.
            if state
                .pending
                .as_ref()
                .is_some_and(|pending| pending.stop_host)
            {
                state.cleanup_retained = true;
            }
            state.pending = None;
            state.revision += 1;
        }
        result
    }
    async fn complete_pending(&self) -> Result<ClosedWindows> {
        // Only this source proof permits bypassing dead clients. A ready error,
        // empty registry or finished bootstrap thread is never enough.
        loop {
            if self.startup_outcome().await? == HostEndpointStartupOutcome::NoOwnerClean {
                break;
            }
            let next = {
                let state = self.state()?;
                let pending = state.pending.as_ref().context("Missing close operation.")?;
                pending
                    .windows
                    .iter()
                    .find(|id| !pending.detached.contains(id))
                    .map(|id| (*id, state.windows[id].client.clone()))
            };
            let Some((id, client)) = next else { break };
            // DetachAll is per UUID and idempotent. Cancellation can only repeat
            // this release, never input, takeover, PTY close or a new controller.
            if let Err(error) = self.shared.backend.detach_window(&client).await {
                // The actor may have completed explicit cleanup while a release
                // was in flight. Do not trap a window on its now-dead client.
                if self.startup_outcome().await? != HostEndpointStartupOutcome::NoOwnerClean {
                    return Err(error);
                }
                break;
            }
            self.state()?
                .pending
                .as_mut()
                .context("Missing close operation.")?
                .detached
                .insert(id);
        }
        let stop_host = self
            .state()?
            .pending
            .as_ref()
            .context("Missing close operation.")?
            .stop_host;
        let shutdown_outcome = if stop_host {
            self.shared.backend.stop().await?
        } else {
            None
        };
        let mut state = self.state()?;
        let pending = state.pending.take().context("Missing close operation.")?;
        for id in &pending.windows {
            state.windows.remove(id);
        }
        if stop_host {
            // Retain the typed report before publishing stopped. Dirty activity
            // is a persistence warning, not failed resource cleanup or a commit.
            state.shutdown_outcome = shutdown_outcome;
            if let Some(outcome) = &state.shutdown_outcome {
                if let ActivityShutdownState::DirtyAfterOwnershipLoss { pending_sessions } =
                    &outcome.activity
                {
                    // Logging failure never discards the whole retained report.
                    let _ = writeln!(
                        std::io::stderr().lock(),
                        "Warning: host resources are confirmed closed; {pending_sessions} session activity updates were not committed after ownership loss."
                    );
                }
                if let shellfox_service::PresentationShutdownState::FailedSources(warnings) =
                    &outcome.presentation
                {
                    let count = warnings.failures().len();
                    let _ = writeln!(
                        std::io::stderr().lock(),
                        "Warning: host resources are confirmed closed; {count} source presentations failed. Opaque source evidence remains retained; final view completeness is not claimed."
                    );
                }
                // NoKnownFailure is not a positive final-view completeness proof.
            }
            state.stopped = true;
            state.cleanup_retained = false;
        }
        state.revision += 1;
        Ok(ClosedWindows {
            windows: pending.windows,
            app_stopped: stop_host,
        })
    }
}
