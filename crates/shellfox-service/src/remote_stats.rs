use super::*;
impl RemoteService<'_> {
    pub(super) fn statistics(
        &self,
        input: wire::GetSessionStats,
    ) -> Result<serde_json::Value, AppError> {
        let snapshot = self.service.host().snapshot().map_err(host_error)?;
        let ids = if let Some(id) = input.session_id {
            vec![id]
        } else {
            snapshot.sessions.iter().map(|session| session.id).collect()
        };
        let mut sessions = Vec::new();
        let mut tabs = Vec::new();
        for id in ids {
            let mut session = self.session(&snapshot, id)?;
            session.env.clear();
            for tab in &session.tabs {
                tabs.push(wire::TabStat {
                    tab_id: tab.id,
                    processes: Vec::new(),
                    memory_bytes: None,
                    elapsed_seconds: None,
                    reason: Some(
                        "Authoritative process/resource measurements are unavailable.".into(),
                    ),
                });
            }
            sessions.push(session);
        }
        if tabs.len() > 64 {
            return Err(capability_unavailable("complete statistics inventory exceeds the bounded response; no partial rows were returned"));
        }
        checked(wire::SessionStats { sessions, tabs })
    }
}
