use super::*;
impl Root {
    pub(crate) fn selected_document_backend(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(DocumentBackend, Option<String>)> {
        if self.is_local_source() {
            let selected_tab = self.selected_tab();
            let session = self.snapshot.as_ref().and_then(|snapshot| {
                snapshot.sessions.iter().find(|session| Some(session.id) == self.selected_session())
            });
            let tab = session.and_then(|session| session.tabs.iter().find(|tab| Some(tab.id) == selected_tab));
            #[cfg(windows)]
            if let Some(tab) = tab.filter(|tab| {
                let profile = tab.profile_id.as_str();
                profile == "wsl" || profile.starts_with("wsl:")
            }) {
                let Some(account) = tab.wsl_account() else {
                    self.error = Some("This terminal has no verified WSL launch user. Open a new WSL tab to use Files.".into());
                    cx.notify();
                    return None;
                };
                return match shellfox_files::WslBrowserScope::for_tab(&tab.cwd, tab.profile_id.as_str()) {
                    Ok(scope) => {
                        let guest = scope.guest_cwd().to_owned();
                        Some((DocumentBackend::local_wsl(scope, tab.id, account.clone()), Some(guest)))
                    }
                    Err(error) => {
                        self.error = Some(format!("WSL Files cannot use this tab's directory: {error}"));
                        cx.notify();
                        None
                    }
                };
            }
            let cwd = tab.map(|tab| &tab.cwd).or_else(|| session.map(|session| &session.cwd));
            return match cx.try_global::<EditorOutcome>() {
                Some(EditorOutcome::Ready(backend)) => Some((DocumentBackend::local(backend), cwd.cloned())),
                outcome => {
                    self.error = Some(files_unavailable(outcome).into());
                    cx.notify();
                    None
                }
            };
        }
        match self.peer_file_backend() {
            Ok(value) => Some(value),
            Err(error) => {
                self.error = Some(format!("Peer Files unavailable: {error:#}"));
                cx.notify();
                None
            }
        }
    }

    pub(crate) fn peer_file_backend(&self) -> anyhow::Result<(DocumentBackend, Option<String>)> {
        use crate::root::{
            peer::metadata::PeerApi,
            source::{PeerCycle, sessions},
        };
        let (peer, state) = self
            .peers
            .selected_peer()
            .ok_or_else(|| anyhow::anyhow!("No peer source is selected."))?;
        let cycle = PeerCycle {
            peer: peer.clone(),
            generation: state.cycle,
        };
        let cwd = state.snapshot.as_ref().and_then(|snapshot| {
            let session =
                sessions(snapshot).find(|session| Some(session.id) == state.selected_session)?;
            session
                .tabs
                .iter()
                .find(|tab| Some(tab.id) == state.selected_tab)
                .and_then(|tab| tab.facts.cwd.known())
                .or_else(|| session.facts.cwd.known())
                .cloned()
        });
        let backend = match self.peer_api(&cycle)? {
            PeerApi::Legacy(binding) => DocumentBackend::legacy(binding)?,
            PeerApi::Manager(connection, admission) => {
                DocumentBackend::peer(connection, admission)?
            }
        };
        Ok((backend, cwd))
    }
}
