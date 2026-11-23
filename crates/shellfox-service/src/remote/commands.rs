use super::*;

impl RemoteService<'_> {
    fn snapshot(&self) -> Result<Snapshot, AppError> {
        self.service.host().snapshot().map_err(host_error)
    }
    pub(super) fn session(&self, snapshot: &Snapshot, id: SessionId) -> Result<wire::SessionDto, AppError> {
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| failure(ErrorCode::NotFound, "Session not found."))?;
        let facts = self
            .state
            .facts
            .sessions
            .get(&id)
            .ok_or_else(|| capability_unavailable("session runtime facts"))?;
        let dto = adapter::session_dto_visible(session, facts, &self.private_tabs(snapshot)).map_err(AppError::from)?;
        adapter::filter_private_tabs(dto, &self.private_tabs(snapshot)).map_err(Into::into)
    }
    pub(super) fn rename_session_metadata(&self, input: wire::RenameSession) -> Result<serde_json::Value, AppError> {
        self.preflight(input.session_id)?;
        let snapshot = self.service.host().rename_session(input.session_id, &input.title).map_err(host_error)?;
        checked(self.session(&snapshot, input.session_id)?)
    }
    pub(super) fn set_session_env_metadata(&self, input: wire::SetSessionEnv) -> Result<serde_json::Value, AppError> {
        self.preflight(input.session_id)?;
        let snapshot = self.service.host().set_session_env(input.session_id, input.env).map_err(host_error)?;
        checked(self.session(&snapshot, input.session_id)?)
    }
    pub(super) fn reorder_session_metadata(&self, input: wire::ReorderTabs) -> Result<serde_json::Value, AppError> {
        self.preflight(input.session_id)?;
        let snapshot = self.snapshot()?;
        let private = self.private_tabs(&snapshot);
        let session = snapshot.sessions.iter().find(|session| session.id == input.session_id).ok_or_else(|| failure(ErrorCode::NotFound, "Session not found."))?;
        let visible = session.tabs.iter().filter(|tab| !private.contains(&tab.id)).map(|tab| tab.id).collect::<std::collections::HashSet<_>>();
        if input.tab_ids.len() != visible.len() || input.tab_ids.iter().any(|id| !visible.contains(id)) { return Err(failure(ErrorCode::NotFound, "Authorized tab order does not match this session.")); }
        let mut requested = input.tab_ids.into_iter();
        let merged = session.tabs.iter().map(|tab| if visible.contains(&tab.id) { requested.next().ok_or_else(|| failure(ErrorCode::Validation, "Authorized tab order is incomplete.")) } else { Ok(tab.id) }).collect::<Result<Vec<_>, AppError>>()?;
        let snapshot = self.service.host().reorder_tabs(input.session_id, &merged).map_err(host_error)?;
        checked(self.session(&snapshot, input.session_id)?)
    }
    fn preflight(&self, id: SessionId) -> Result<(), AppError> {
        self.session(&self.snapshot()?, id).map(|_| ())
    }
    pub(super) fn execute(
        &mut self,
        actor: &Actor,
        command: RemoteCommand,
    ) -> Result<serde_json::Value, AppError> {
        #[cfg(windows)]
        if let Some(value) = self.route_wsl_file(actor, &command)? {
            return Ok(value);
        }
        use RemoteCommand::*;
        match command {
            GetSnapshot(_) => {
                let saved = self.snapshot()?;
                let private = self.private_tabs(&saved);
                let settings = self.service.host().global_settings().map_err(host_error)?;
                let mut facts = self.state.facts.clone();
                facts.explorer.get_or_insert_with(|| wire::ExplorerIntegration {
                    supported: false, installed: false, folder_item_installed: false,
                    background_installed: false, reason: Some("Explorer integration is unavailable on this host.".into()),
                });
                facts.cli.get_or_insert_with(|| wire::CliIntegration {
                    supported: false, installed: false, command: "shellfox".into(),
                    reason: Some("CLI integration is unavailable on this host.".into()),
                });
                let mut snapshot = adapter::manager_snapshot_with_settings_visible(
                    &saved,
                    &facts,
                    &settings,
                    &private,
                )
                .map_err(AppError::from)?;
                snapshot.sessions = snapshot
                    .sessions
                    .into_iter()
                    .map(|session| {
                        adapter::filter_private_tabs(session, &private)
                    })
                    .collect::<Result<_, _>>()
                    .map_err(AppError::from)?;
                checked(snapshot)
            }
            GetTerminalProfiles(_) => {
                let mut profiles = self
                    .state
                    .profiles
                    .clone()
                    .ok_or_else(|| capability_unavailable("terminal profiles"))?;
                profiles
                    .profiles
                    .retain(|profile| profile.elevation.is_none());
                profiles.default_profile_id = super::v1_launch::default_profile(&profiles).map(str::to_owned);
                checked(profiles)
            }
            GetHomeDirectory(_) => checked(
                self.state
                    .home
                    .clone()
                    .ok_or_else(|| capability_unavailable("home directory"))?,
            ),
            SetVisibleSessions(input) => {
                let snapshot = self.snapshot()?;
                if !input
                    .session_ids
                    .iter()
                    .all(|id| snapshot.sessions.iter().any(|s| s.id == *id))
                {
                    return Err(failure(ErrorCode::NotFound, "Visible session not found."));
                }
                self.state
                    .views
                    .get_mut(&key(actor))
                    .ok_or_else(|| failure(ErrorCode::AuthFailed, "View missing."))?
                    .visible = input.session_ids;
                self.state.visibility_dirty = true;
                checked(wire::Configured { configured: true })
            }
            command @ (BeginFileRead(_)
            | ReadFileChunk(_)
            | BeginFileWrite(_)
            | WriteFileChunk(_)
            | FinishFileWrite(_)
            | CancelFileTransfer(_)) => {
                files::transfer(self.service, actor, command, &mut self.state.transfers)
            }
            ResolveDirectory(input) => shellfox_remote::directories::resolve(
                actor,
                &input,
                self.state.home.as_ref().map(|home| home.cwd.as_str()),
            ),
            CompleteDirectory(input) => shellfox_remote::directories::complete(
                actor,
                &input,
                self.state.home.as_ref().map(|home| home.cwd.as_str()),
            ),
            CreateDirectory(input) => files::directory(
                self.service,
                actor,
                input,
                self.state.home.as_ref().map(|home| home.cwd.as_str()),
            ),
            ListFiles(input) => files::list(self.service, actor, input),
            ReadFile(input) => files::read(self.service, actor, input),
            ReadFileAsset(input) => files::asset(self.service, actor, input),
            WriteImageFile(input) => files::image(self.service, actor, input),
            WriteFile(input) => files::write(self.service, actor, input),
            RenameSession(input) => self.rename_session_metadata(input),
            RenameTab(input) => {
                self.preflight(input.session_id)?;
                self.tab_member(input.session_id, input.tab_id)?;
                let snapshot = self
                    .service
                    .host()
                    .rename_tab(input.tab_id, &input.title)
                    .map_err(host_error)?;
                checked(self.session(&snapshot, input.session_id)?)
            }
            ReorderTabs(input) => self.reorder_session_metadata(input),
            SetSessionPinned(input) => {
                self.preflight(input.session_id)?;
                let snapshot = self
                    .service
                    .host()
                    .set_session_pinned(input.session_id, input.pinned)
                    .map_err(host_error)?;
                checked(self.session(&snapshot, input.session_id)?)
            }
            SettleSession(input) => {
                self.preflight(input.session_id)?;
                let snapshot = self
                    .service
                    .host()
                    .set_session_settled(input.session_id, true)
                    .map_err(host_error)?;
                checked(self.session(&snapshot, input.session_id)?)
            }
            UnsettleSession(input) => {
                self.preflight(input.session_id)?;
                let snapshot = self
                    .service
                    .host()
                    .set_session_settled(input.session_id, false)
                    .map_err(host_error)?;
                checked(self.session(&snapshot, input.session_id)?)
            }
            SetSessionEnv(input) => self.set_session_env_metadata(input),
            SetTerminalEnv(input) => {
                let previous = self.service.host().global_settings().map_err(host_error)?;
                let patch = shellfox_core::SettingsPatch {
                    terminal_env: Some(input.env),
                    ..Default::default()
                };
                let candidate = previous.patched(patch.clone()).map_err(host_error)?;
                adapter::persisted_settings(&candidate).map_err(AppError::from)?;
                let settings = self
                    .service
                    .host()
                    .save_global_settings(patch)
                    .map_err(host_error)?;
                checked(adapter::persisted_settings(&settings).map_err(AppError::from)?)
            }
            CloseTab(input) => {
                let snapshot = self.snapshot()?;
                let session_id = snapshot
                    .sessions
                    .iter()
                    .find(|session| session.tabs.iter().any(|tab| tab.id == input.tab_id))
                    .map(|session| session.id)
                    .ok_or_else(|| failure(ErrorCode::NotFound, "Tab not found."))?;
                self.preflight(session_id)?;
                let current = self
                    .service
                    .terminals
                    .get(&input.tab_id)
                    .map(|terminal| terminal.runtime.identity().generation);
                if current != Some(input.generation) {
                    return Err(failure(ErrorCode::TargetLost, "Stale terminal generation."));
                }
                let snapshot = self.service.close_tab(input.tab_id).map_err(|_| {
                    failure(
                        ErrorCode::NativeUnavailable,
                        "Owned cleanup not confirmed; metadata retained.",
                    )
                })?;
                checked(self.session(&snapshot, session_id)?)
            }
            DeleteSession(input) => {
                self.service.delete_session(input.session_id).map_err(|_| {
                    failure(
                        ErrorCode::NativeUnavailable,
                        "Owned terminal cleanup not confirmed; metadata retained.",
                    )
                })?;
                self.state.facts.sessions.remove(&input.session_id);
                checked(wire::Deleted { deleted: true })
            }
            CreateSession(input) => self.create_session(input),
            AddTab(input) => self.add_tab(input),
            ActivateSession(input) => self.activate_session(input),
            command @ (AttachTerminal(_)
            | TakeTerminalControl(_)
            | WriteTerminal(_)
            | ResizeTerminal(_)
            | AcknowledgeTerminal(_)
            | DetachTerminal(_)) => self.terminal_operation(actor, command),
            ClearSessionError(input) => {
                self.preflight(input.session_id)?;
                let snapshot = self.service.acknowledge_session_error(input.session_id)
                    .map_err(|_| failure(ErrorCode::NativeUnavailable, "Could not acknowledge the session error."))?;
                checked(self.session(&snapshot, input.session_id)?)
            }
            GetSessionStats(input) => self.statistics(input),
            BeginTerminalUpload(_)
            | WriteTerminalUpload(_)
            | FinishTerminalUpload(_)
            | CancelTerminalUpload(_) => Err(capability_unavailable(
                "legacy terminal uploads",
            )),
        }
    }
    fn tab_member(&self, session: SessionId, tab: shellfox_core::TabId) -> Result<(), AppError> {
        if !self
            .snapshot()?
            .sessions
            .iter()
            .any(|s| s.id == session && s.tabs.iter().any(|t| t.id == tab))
        {
            return Err(failure(
                ErrorCode::NotFound,
                "Tab does not belong to this session.",
            ));
        }
        Ok(())
    }
}
