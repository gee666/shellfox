//! Factless ORIGINAL intent for a new SSH program, not a local-shell substitution.
use super::native::NativeSshOwner;
use super::*;
use crate::Service;
use anyhow::{ensure, Context};
use shellfox_core::{ProfileId, SessionId, Snapshot, TabId};
use shellfox_host::{ReservedPublicationReceipt, ReservedSessionPublication};

#[derive(Clone, Debug)]
pub struct OpenProfile {
    pub request_id: SessionId,
    pub profile: Uuid,
    pub cols: u16,
    pub rows: u16,
}
#[derive(Clone, Debug)]
pub struct OpenedProfile {
    pub snapshot: Snapshot,
    pub connection: ConnectionStatus,
}
impl NativeSshOwner {
    pub fn open_profile(
        &mut self,
        service: &mut Service,
        controller: u64,
        input: OpenProfile,
    ) -> Result<OpenedProfile> {
        service.assert_accepting()?;
        ensure!(
            self.grants.contains(&controller),
            "Native SSH authority is unavailable."
        );
        RemotePty::new(input.cols, input.rows)?;
        let state = self
            .installed
            .as_ref()
            .context("SSH services not installed")?;
        let saved = state
            .manager
            .profiles
            .get_for_connection(input.profile)?
            .context("SSH profile not found")?;
        let profile_id = ProfileId::new(format!("ssh-{}", input.profile))?;
        let (session, tab, existing) = match service
            .host
            .reserved_publication_receipt(input.request_id)?
        {
            ReservedPublicationReceipt::Published { session_id, tab_id } => {
                (session_id, tab_id, true)
            }
            ReservedPublicationReceipt::TargetDeleted { .. } => {
                anyhow::bail!("The original SSH target was deleted; a retry cannot recreate it.")
            }
            ReservedPublicationReceipt::ReceiptUnavailable => {
                (SessionId::new(), TabId::new(), false)
            }
        };
        let snapshot = service.host.publish_reserved_session(
            ReservedSessionPublication {
                request_id: input.request_id,
                session_id: session,
                tab_id: tab,
                title: Some(saved.name),
                cwd: service
                    .isolation
                    .to_str()
                    .context("Unsupported isolation path")?
                    .into(),
                profile_id: profile_id.clone(),
            },
            None,
        )?;
        let target = snapshot
            .sessions
            .iter()
            .find(|s| s.id == session)
            .and_then(|s| s.tabs.iter().find(|t| t.id == tab))
            .context("Original SSH target is unavailable")?;
        ensure!(
            target.profile_id == profile_id,
            "OpenProfile receipt belongs to another original intent."
        );
        if existing {
            let current = self
                .installed
                .as_ref()
                .and_then(|s| s.manager.connections.values().find(|c| c.status.tab == tab));
            let c=current.context("Original SSH request has no current connection receipt; do not automatically restart it.")?;
            ensure!(
                c.controller == controller || c.authenticated,
                "Original SSH approval belongs to another application view."
            );
            let mut connection = c.status.clone();
            self.project_status(service, controller, &mut connection);
            return Ok(OpenedProfile {
                snapshot,
                connection,
            });
        }
        // No actor work can interleave publication and source reservation.
        // An opaque ssh-* profile also cannot fall back through the local catalog.
        let Response::Status(mut statuses) = self.handle(
            service,
            controller,
            Request::Connect {
                profile: input.profile,
                tab,
                cols: input.cols,
                rows: input.rows,
            },
        )?
        else {
            anyhow::bail!("SSH connect returned no status")
        };
        let mut connection = statuses.pop().context("SSH connection status missing")?;
        self.project_status(service, controller, &mut connection);
        Ok(OpenedProfile {
            snapshot,
            connection,
        })
    }
}
