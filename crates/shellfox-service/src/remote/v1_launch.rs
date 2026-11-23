use super::*;
use shellfox_core::{ProfileId, TabState};
use shellfox_host::CreationRequestReceipt;

pub(super) fn default_profile(profiles: &wire::TerminalProfiles) -> Option<&str> {
    let saved = profiles.default_profile_id.as_deref().unwrap_or_default();
    crate::effective_default_profile(saved, crate::usable_profiles(&profiles.profiles))
}

pub(super) fn ensure_launch_profile(state: &RemoteState, id: &str) -> Result<(), AppError> {
    if super::privacy::elevated_profile(id) {
        return Err(failure(
            ErrorCode::AccessDenied,
            "Elevated launch remains local-only.",
        ));
    }
    let profiles = state
        .profiles
        .as_ref()
        .ok_or_else(|| capability_unavailable("trusted manager launch profiles"))?;
    profiles
        .validate()
        .map_err(|_| failure(ErrorCode::Internal, "Invalid native profile evidence."))?;
    let profile = profiles
        .profiles
        .iter()
        .find(|profile| profile.id == id)
        .ok_or_else(|| failure(ErrorCode::NotFound, "Authorized launch profile not found."))?;
    if profile.elevation.is_some() {
        return Err(failure(
            ErrorCode::AccessDenied,
            "Elevated launch remains local-only.",
        ));
    }
    if !profile.available {
        return Err(capability_unavailable("requested native shell launch"));
    }
    Ok(())
}

impl RemoteService<'_> {
    fn default_v1_profile(&self) -> Result<ProfileId, AppError> {
        let id = self
            .state
            .profiles
            .as_ref()
            .and_then(default_profile)
            .ok_or_else(|| capability_unavailable("available host-account terminal profile"))?;
        ProfileId::new(id).map_err(host_error)
    }

    fn launched_session(&self, id: SessionId) -> Result<serde_json::Value, AppError> {
        let snapshot = self.service.host().snapshot().map_err(host_error)?;
        let private = self.private_tabs(&snapshot);
        let facts = crate::activity::integration::SourceFacts {
            sequence: 0,
            snapshot,
            settings: self.service.host().global_settings().map_err(host_error)?,
            terminals: self
                .service
                .terminals
                .values()
                .map(|t| crate::facts::capture::terminal(self.service, t))
                .collect(),
            creation_facts: self
                .service
                .host()
                .session_creation_facts()
                .map_err(host_error)?,
            #[cfg(windows)]
            wsl_profiles: self.service.catalog.wsl.cache.current(),
            cancelled: self.service.ownership_lost.clone(),
        };
        let projected =
            crate::facts::projection::project_session_facts_visible(&facts, Some(id), &private)?;
        let session = facts
            .snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(missing)?;
        let session_facts = projected.get(&id).ok_or_else(missing)?;
        let dto = adapter::session_dto_visible(session, session_facts, &private)
            .map_err(AppError::from)?;
        checked(adapter::filter_private_tabs(dto, &private).map_err(AppError::from)?)
    }

    fn start_remote_tab(&mut self, tab: &shellfox_core::Tab) -> Result<(), AppError> {
        if !self.service.terminals.contains_key(&tab.id) {
            super::v1_privilege::ensure_normal_launch()?;
            ensure_launch_profile(self.state, tab.profile_id.as_str())?;
            self.service.start_terminal(tab.id).map_err(|_| {
                failure(
                    ErrorCode::NativeUnavailable,
                    "Could not start the terminal.",
                )
            })?;
        }
        Ok(())
    }

    pub(super) fn create_session(
        &mut self,
        input: wire::CreateSession,
    ) -> Result<serde_json::Value, AppError> {
        let host = self.service.host();
        let receipt = host
            .creation_request_receipt(input.request_id)
            .map_err(host_error)?;
        if let CreationRequestReceipt::SessionCreated { session_id } = receipt {
            return self.launched_session(session_id);
        }
        super::v1_privilege::ensure_normal_launch()?;
        let profile = if cfg!(windows) {
            let configured = host
                .snapshot()
                .map_err(host_error)?
                .preferences
                .default_profile_id;
            creation_profile::for_windows_folder(self.state, &configured, &input.cwd)?
        } else {
            self.default_v1_profile()?
        };
        ensure_launch_profile(self.state, profile.as_str())?;
        let snapshot = self
            .service
            .create_application_session(shellfox_core::CreateSession {
                request_id: input.request_id,
                title: input.title,
                cwd: input.cwd,
                profile_id: profile,
            })
            .map_err(|_| failure(ErrorCode::NativeUnavailable, "Could not create a session."))?;
        let CreationRequestReceipt::SessionCreated { session_id } = self
            .service
            .host()
            .creation_request_receipt(input.request_id)
            .map_err(host_error)?
        else {
            return Err(missing());
        };
        if let Some(tab) = snapshot
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .and_then(|s| s.tabs.first())
        {
            self.start_remote_tab(tab)?;
        }
        self.launched_session(session_id)
    }

    pub(super) fn add_tab(&mut self, input: wire::AddTab) -> Result<serde_json::Value, AppError> {
        let host = self.service.host();
        let before = host.snapshot().map_err(host_error)?;
        let session = before
            .sessions
            .iter()
            .find(|s| s.id == input.session_id)
            .ok_or_else(missing)?;
        if session
            .tabs
            .iter()
            .any(crate::automation::replay::is_automation_tab)
        {
            return Err(failure(
                ErrorCode::Unsupported,
                "Automation views cannot open shell tabs.",
            ));
        }
        let profile = match input.profile_id {
            Some(id) => ProfileId::new(id).map_err(host_error)?,
            None => self.default_v1_profile()?,
        };
        super::v1_privilege::ensure_normal_launch()?;
        ensure_launch_profile(self.state, profile.as_str())?;
        let updated = host
            .add_tab(
                session.id,
                shellfox_core::AddTab {
                    title: input.title,
                    cwd: input.cwd,
                    profile_id: profile,
                },
            )
            .map_err(host_error)?;
        let tab = updated
            .sessions
            .iter()
            .find(|s| s.id == session.id)
            .and_then(|s| {
                s.tabs
                    .iter()
                    .find(|tab| !session.tabs.iter().any(|old| old.id == tab.id))
            })
            .ok_or_else(missing)?;
        self.start_remote_tab(tab)?;
        self.launched_session(session.id)
    }

    pub(super) fn activate_session(
        &mut self,
        input: wire::ActivateSession,
    ) -> Result<serde_json::Value, AppError> {
        let snapshot = self.service.host().snapshot().map_err(host_error)?;
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.id == input.session_id)
            .ok_or_else(missing)?;
        let private = self.private_tabs(&snapshot);
        if let Some(tab) = session.tabs.iter().find(|t| {
            t.state != TabState::Archived
                && !private.contains(&t.id)
                && !crate::automation::replay::is_automation_tab(t)
        }) {
            self.start_remote_tab(tab)?;
        }
        self.launched_session(session.id)
    }
}

fn missing() -> AppError {
    failure(ErrorCode::NotFound, "Session or terminal tab not found.")
}
