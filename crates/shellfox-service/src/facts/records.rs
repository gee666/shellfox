//! The host's sessions as 0.9's v1 records, the shape a peer reads, for the
//! local CLI: one adapter, so local and `--host` output agree.
use crate::{activity::integration::SourceFacts, Service};
use anyhow::Result;

impl Service {
    /// Every representable session, all tabs included: the local account owns them.
    pub(crate) fn session_records(&self) -> Result<Vec<shellfox_protocol::SessionDto>> {
        let host = self.host();
        let source = SourceFacts {
            sequence: 0,
            snapshot: host.snapshot()?,
            settings: host.global_settings()?,
            terminals: self
                .terminals
                .values()
                .map(|t| super::capture::terminal(self, t))
                .collect(),
            creation_facts: host.session_creation_facts()?,
            #[cfg(windows)]
            wsl_profiles: self.catalog.wsl.cache.current(),
            cancelled: self.ownership_lost.clone(),
        };
        let private = Default::default();
        let projected = super::projection::project_legacy_catalog(&source, None, &private)
            .map_err(|error| anyhow::anyhow!(error.message))?;
        source
            .snapshot
            .sessions
            .iter()
            .filter(|s| !projected.omitted_sessions.contains(&s.id))
            .map(|session| {
                let facts = projected
                    .sessions
                    .get(&session.id)
                    .ok_or_else(|| anyhow::anyhow!("Session runtime facts are missing."))?;
                shellfox_remote::adapter::session_dto(session, facts)
                    .map_err(|error| anyhow::anyhow!("{error:?}"))
            })
            .collect()
    }
}
