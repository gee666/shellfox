use crate::activity::integration::{ActivityFlush, InspectionScope, SourceFactsWatch};
use crate::*;
#[cfg(test)]
mod tests;
use shellfox_runtime::{Identity, RuntimeHooks};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivityShutdownState {
    Committed,
    DirtyAfterOwnershipLoss { pending_sessions: usize },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShutdownOutcome {
    pub activity: ActivityShutdownState,
    pub presentation: crate::PresentationShutdownState,
}
#[derive(Debug)]
pub enum ShutdownFailure {
    ResourceCleanup(anyhow::Error),
    ActivityPersistence(anyhow::Error),
}
impl std::fmt::Display for ShutdownFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ResourceCleanup(_) => {
                "Owned resource cleanup is unconfirmed; controller retained."
            }
            Self::ActivityPersistence(_) => {
                "Owned processes stopped but activity persistence failed; pending touches retained."
            }
        })
    }
}
impl std::error::Error for ShutdownFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::ResourceCleanup(error) | Self::ActivityPersistence(error) => error.as_ref(),
        })
    }
}
impl Service {
    pub fn source_facts_watch(&self) -> SourceFactsWatch {
        self.activity.watch.clone()
    }
    pub(crate) fn register_activity(
        &mut self,
        identity: Identity,
        config: &shellfox_terminal::PtyConfig,
    ) -> Result<RuntimeHooks> {
        let profile = config.profile.and_then(|id| {
            shellfox_terminal::discover_shell_profiles()
                .ok()?
                .into_iter()
                .find(|profile| profile.id() == id)
        });
        let expected = profile
            .as_ref()
            .and_then(|profile| profile.executable())
            .and_then(|path| path.to_str())
            .unwrap_or("unsupported-owned-shell")
            .to_owned();
        let scope = if profile.is_none() {
            InspectionScope::Unsupported
        } else if config.profile == Some(shellfox_terminal::ShellProfileId::Wsl) {
            InspectionScope::Guest
        } else if cfg!(windows) {
            InspectionScope::LocalWindows
        } else if cfg!(target_os = "linux") {
            InspectionScope::LocalUnix
        } else {
            InspectionScope::Unsupported
        };
        self.activity
            .owner
            .watch(identity, expected, scope)
            .map_err(|_| anyhow::anyhow!("Owned activity watch unavailable."))?;
        let live_output = self
            .activity
            .owner
            .live_output_sink(identity)
            .map(|sink| sink as Arc<dyn shellfox_runtime::LiveOutputSink>);
        self.activity.dirty = true;
        Ok(RuntimeHooks { live_output })
    }
    pub(crate) fn activity_input_reply(
        &mut self,
        identity: Identity,
        request: shellfox_runtime::RequestId,
        result: &std::result::Result<shellfox_runtime::Response, shellfox_runtime::RuntimeError>,
    ) {
        let Some(sink) = self.activity.owner.live_output_sink(identity) else {
            return;
        };
        // finish consumes/removes before a following Closed event in this drain
        // batch. Its mutex is released before any Host/CAS call below.
        let finished = sink.finish_input(identity, request, result);
        match finished {
            Ok(Some(accepted)) if accepted.activity.touch_session => {
                let Ok(now) = self.activity.now_ms() else {
                    return;
                };
                if self.ownership_lost.is_cancelled() {
                    let _ = self.activity.persistence.touch(identity.session_id, now);
                } else {
                    let metadata = self.activity.persistence.accepted_input(
                        &self.host,
                        identity,
                        accepted.activity,
                        accepted.admission.captured_settlement,
                        now,
                    );
                    self.activity.dirty |= metadata.resumed || metadata.metadata_error.is_some();
                }
            }
            Err(_) => self.activity.dirty = true,
            _ => {}
        }
    }
    pub(crate) fn cancel_activity_inputs(&self, identity: Identity) {
        if let Some(sink) = self.activity.owner.live_output_sink(identity) {
            sink.cancel_all_inputs(identity);
        }
    }
    pub(crate) fn activity_collect_live(&mut self) {
        self.activity.owner.sync_live_status(&self.terminals);
        let updates: Vec<_> = self
            .activity
            .owner
            .take_live_updates()
            .into_iter()
            .filter(|update| {
                self.terminals
                    .get(&update.identity.tab_id)
                    .is_some_and(|terminal| terminal.runtime.identity() == update.identity)
            })
            .collect();
        // ALL live data touches TTL. The shared filter controls BUSY only.
        let _ = self.activity.persistence.live_updates(&updates);
        self.activity.dirty |= updates
            .iter()
            .any(|update| update.busy_changed.is_some() || !update.available);
        let closed: Vec<_> = self
            .terminals
            .values()
            .filter(|terminal| terminal.runtime.source_kind()==shellfox_runtime::SourceKind::OwnedPty && terminal.runtime.phase() == Phase::Closed)
            .map(|terminal| terminal.runtime.identity())
            .collect();
        for identity in closed {
            self.activity_confirmed_exit(identity);
        }
    }
    pub(crate) fn activity_confirmed_exit(&mut self, identity: Identity) {
        if self.activity.owner.confirmed_exit(identity).touch_session {
            if let Ok(now) = self.activity.now_ms() {
                let _ = self.activity.persistence.touch(identity.session_id, now);
            }
            self.activity.dirty = true;
        }
    }
    pub(crate) fn flush_activity(&mut self, force: bool) -> Result<()> {
        self.activity_collect_live();
        let now = self.activity.now_ms()?;
        self.assert_owned()?;
        let budget = if force {
            self.activity.persistence.pending_count().max(1)
        } else {
            1
        };
        let mut deferred = false;
        for _ in 0..budget {
            match self.activity.persistence.flush_next(&self.host, now, force) {
                ActivityFlush::Idle => break,
                ActivityFlush::Committed { .. } => self.activity.dirty = true,
                ActivityFlush::Deferred { .. } => deferred = true,
            }
        }
        if deferred {
            bail!("Activity metadata commit deferred; pending touches retained.");
        }
        Ok(())
    }
    pub(crate) fn pump_activity(&mut self) {
        self.activity_collect_live();
        let Ok(now) = self.activity.now_ms() else {
            self.activity.publisher.invalidate();
            return;
        };
        if !self.activity.owner.tick(now).is_empty() {
            self.activity.dirty = true;
        }
        if self.ownership_lost.is_cancelled() {
            self.activity.publisher.invalidate();
            return;
        }
        let _ = self.flush_activity(false);
        if now < self.activity.next_poll_ms {
            return;
        }
        self.activity.next_poll_ms = now.saturating_add(500);
        let capture = (|| -> Result<()> {
            let settings = self.host.global_settings()?;
            if self.activity.rules != settings.process_rules {
                self.activity.owner.rules_changed();
                self.activity.rules = settings.process_rules.clone();
                self.activity.dirty = true;
            }
            self.activity.apply_visibility()?;
            let updates = self.activity.owner.poll_processes(
                &self.terminals,
                &self.activity.rules,
                now,
                4,
                false,
            );
            self.activity.dirty |= !updates.is_empty();
            for update in updates {
                if update.touch_session {
                    let _ = self
                        .activity
                        .persistence
                        .touch(update.identity.session_id, now);
                }
            }
            let snapshot = self.host.snapshot()?;
            let current_revision = crate::activity::integration::read_source(&self.activity.watch)
                .map(|source| source.snapshot.revision);
            if self.activity.dirty || current_revision != Some(snapshot.revision) {
                self.publish_source_facts(snapshot, settings)?;
            }
            Ok(())
        })();
        if capture.is_err() {
            self.activity.publisher.invalidate();
        }
    }
    pub(crate) fn publish_source_facts(
        &mut self,
        snapshot: shellfox_core::Snapshot,
        settings: shellfox_core::Settings,
    ) -> Result<()> {
        let creation_facts = self.host.session_creation_facts().inspect_err(|_| {
            self.activity.publisher.invalidate();
        })?;
        let terminals = self
            .terminals
            .values()
            .filter(|terminal| {
                crate::facts::membership::contains(&snapshot, terminal.runtime.identity())
            })
            .map(|terminal| crate::facts::capture::terminal(self, terminal))
            .collect();
        #[cfg(windows)]
        self.activity.publisher.set_wsl_profiles(self.catalog.wsl.cache.current());
        self.activity
            .publisher
            .publish_with_creation_facts(
                snapshot,
                settings,
                terminals,
                creation_facts,
                self.ownership_lost.clone(),
            )
            .map_err(|_| anyhow::anyhow!("Owned source facts invalid."))?;
        self.activity.dirty = false;
        Ok(())
    }
    pub(crate) fn acknowledge_session_error(&mut self, id: shellfox_core::SessionId) -> Result<shellfox_core::Snapshot> {
        let snapshot = self.host.snapshot()?;
        anyhow::ensure!(snapshot.sessions.iter().any(|session| session.id == id), "Session not found.");
        let settings = self.host.global_settings()?;
        for terminal in self.terminals.values_mut() {
            let identity = terminal.runtime.identity();
            if identity.session_id == id && crate::facts::membership::contains(&snapshot, identity)
                && terminal.startup.as_ref().is_some_and(Result::is_err) {
                terminal.startup_error_acknowledged = true;
            }
        }
        // Clear only the presentation error. Runtime phase, cleanup proof and
        // the original startup result must remain authoritative.
        self.publish_source_facts(snapshot.clone(), settings)?;
        Ok(snapshot)
    }

    pub(crate) fn forget_activity(&mut self, identity: Identity) {
        self.cancel_activity_inputs(identity);
        self.activity.owner.forget(identity);
        self.activity.dirty = true;
    }
}
