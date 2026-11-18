use super::*;
use crate::{automation::AutomationViewer, desktop_launch::*};
use shellfox_host::{ReservedPublicationReceipt, ReservedSessionPublication};

pub const AUTOMATION_PROFILE_ID: &str = "automation-pipe";
pub use AUTOMATION_PROFILE_ID as AUTOMATION_READONLY_PROFILE;
struct ShowAuthority<'a> {
    lost: &'a shellfox_protocol::CancellationToken,
    stopping: bool,
}
impl LaunchAuthority for ShowAuthority<'_> {
    fn check_owned(&self) -> std::result::Result<(), LaunchError> {
        if self.lost.is_cancelled() || self.stopping {
            Err(LaunchError::StaleOwner)
        } else {
            Ok(())
        }
    }
}
struct Viewer<'a> {
    host: &'a Host,
    relays: &'a HashMap<Uuid, Relay>,
    terminals: &'a HashMap<TabId, terminals::OwnedTerminal>,
    launches: &'a mut DesktopLaunchRouter,
    authority: ShowAuthority<'a>,
    activity: &'a mut crate::activity_integration::OwnerActivity,
}
impl Viewer<'_> {
    fn identity(&self, id: Uuid) -> Result<Identity> {
        self.authority.check_owned()?;
        let identity = self
            .relays
            .get(&id)
            .context("Automation source reservation is unavailable.")?
            .feed
            .identity();
        let runtime = &self
            .terminals
            .get(&identity.tab_id)
            .context("Automation canonical parser is unavailable.")?
            .runtime;
        if runtime.identity() != identity || runtime.source_kind() != SourceKind::ExternalBytes {
            bail!("Automation canonical source identity or generation changed.");
        }
        Ok(identity)
    }
}
impl AutomationViewer for Viewer<'_> {
    fn create_read_only(&mut self, run: &AutomationRun) -> Result<(Uuid, Uuid)> {
        let identity = self.identity(run.id)?;
        let publication = ReservedSessionPublication {
            request_id: run.id.to_string().parse()?,
            session_id: identity.session_id,
            tab_id: identity.tab_id,
            title: Some(run.title.clone()),
            cwd: run.cwd.clone(),
            profile_id: shellfox_core::ProfileId::new(AUTOMATION_READONLY_PROFILE)?,
        };
        let gate = self
            .relays
            .get(&run.id)
            .context("Automation activity source is missing.")?
            .activity
            .clone();
        let host = self.host;
        gate.publish(self.activity, std::ffi::OsStr::new(&run.executable), || {
            host.publish_reserved_session(publication, None)
                .map_err(anyhow::Error::from)
        })?;
        // Arbitrary raw executables have no verified shell/catalog selection.
        // Publication carries no invented shell facts or PTY registration.
        Ok((
            identity.session_id.to_string().parse()?,
            identity.tab_id.to_string().parse()?,
        ))
    }
    fn focus(&mut self, session: Uuid) -> Result<()> {
        let id = self
            .relays
            .iter()
            .find(|(_, relay)| relay.feed.identity().session_id.to_string() == session.to_string())
            .map(|(id, _)| *id)
            .context("Automation session is not owned by this source.")?;
        let identity = self.identity(id)?;
        match self.host.reserved_publication_receipt(id.to_string().parse()?)? {
            ReservedPublicationReceipt::Published { session_id, tab_id } if session_id == identity.session_id && tab_id == identity.tab_id => {}
            _ => bail!("Original automation view metadata is unavailable; no recreation or selection was inferred."),
        }
        // Explicit Show targets one registered native coordinator. Metadata
        // publication itself never selects/focuses every application window.
        self.launches.enqueue(
            self.launches.owner(),
            DesktopLaunchIntent::new(
                LaunchAction::OpenSession {
                    session: identity.session_id,
                },
                LaunchDisposition::ExistingWindow,
            ),
            &self.authority,
        )?;
        Ok(())
    }
}
impl Service {
    pub(super) fn show_automation(
        &mut self,
        request: AutomationShow,
        launches: &mut DesktopLaunchRouter,
    ) -> Result<AutomationRun> {
        self.assert_accepting()?;
        let authority = ShowAuthority {
            lost: &self.ownership_lost,
            stopping: self.stopping,
        };
        if !launches.route_info(&authority)?.desktop_registered {
            bail!("No native desktop is registered for automation Show.");
        }
        if self.automation.runs.is_stopped(request.id)? {
            if let Some(relay) = self.automation.relays.get(&request.id) {
                let normal = self
                    .terminals
                    .get(&relay.feed.identity().tab_id)
                    .is_some_and(|terminal| relay.feed.finished(&terminal.runtime));
                if normal
                    || self
                        .automation
                        .runs
                        .failed_source_cleanup(request.id)
                        .is_some()
                {
                    let _ = relay.activity.raw_exit_observed(Instant::now());
                }
            }
        }
        let AutomationState { runs, relays, .. } = &mut self.automation;
        let mut viewer = Viewer {
            host: &self.host,
            relays,
            terminals: &self.terminals,
            launches,
            authority,
            activity: &mut self.activity.owner,
        };
        let result = runs.show(request, &mut viewer);
        self.activity.dirty = true;
        result
    }
}
