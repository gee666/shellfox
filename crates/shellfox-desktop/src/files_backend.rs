use super::*;
impl Root {
    pub(super) fn selected_document_backend(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(DocumentBackend, Option<String>)> {
        if self.is_local_source() {
            return match cx.try_global::<EditorOutcome>() {
                Some(EditorOutcome::Ready(backend)) => {
                    Some((DocumentBackend::local(backend), None))
                }
                outcome => {
                    self.error = Some(files_unavailable(outcome).into());
                    cx.notify();
                    None
                }
            };
        }
        #[cfg(feature = "experimental-shellfox-native")]
        {
            let result = self
                .peer_context()
                .and_then(|(cycle, connection, admission)| {
                    // Selector data from the ORIGINAL peer catalog, never authority and never this
                    // viewer's cwd/home. Actual requests remain under the retained admission.
                    let cwd = self.peers.peers.get(&cycle.peer).and_then(|state| {
                        let snapshot = state.snapshot.as_ref()?;
                        crate::root::source::sessions(snapshot)
                            .find(|session| Some(session.id) == state.selected_session)
                            .and_then(|session| session.facts.cwd.known().cloned())
                    });
                    DocumentBackend::peer(connection, admission).map(|backend| (backend, cwd))
                });
            match result {
                Ok(value) => Some(value),
                Err(error) => {
                    self.error = Some(format!("Peer Files unavailable: {error:#}"));
                    cx.notify();
                    None
                }
            }
        }
        #[cfg(not(feature = "experimental-shellfox-native"))]
        {
            self.error = Some("Peer document backend is unavailable.".into());
            cx.notify();
            None
        }
    }
}
