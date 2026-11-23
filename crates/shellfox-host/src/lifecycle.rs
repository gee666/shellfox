use crate::{
    sessions::{ensure_not_prepared, remove_session},
    storage,
    tabs::remove_tab,
    *,
};

impl Host {
    /// Register before launch, including owners retained after startup failure.
    /// A generation stays registered until its adapter confirms owned cleanup.
    pub fn register_terminal(&self, identity: TerminalIdentity) -> Result<()> {
        let mut inner = self.lock()?;
        let data = storage::read(&inner.db)?;
        let tab = data.tab(identity.tab_id)?;
        ensure_not_prepared(&inner.registry, &[tab.id], Some(tab.session_id))?;
        if inner.registry.live.contains_key(&identity.tab_id) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "A terminal owner is already registered.",
            ));
        }
        inner
            .registry
            .live
            .insert(identity.tab_id, identity.generation);
        Ok(())
    }
    /// The local launch owner records the verified account once, for its registered generation.
    pub fn record_wsl_launch_account(
        &self,
        identity: TerminalIdentity,
        account: WslLaunchAccount,
    ) -> Result<()> {
        self.update(ChangeReason::Sessions, |data, registry| {
            if registry.live.get(&identity.tab_id) != Some(&identity.generation) {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Terminal generation is no longer current.",
                ));
            }
            let tab = data.tab_mut(identity.tab_id)?;
            ensure_not_prepared(registry, &[tab.id], Some(tab.session_id))?;
            match &tab.wsl_launch {
                Some(WslLaunchBinding::Pending) => {
                    tab.wsl_launch = Some(WslLaunchBinding::Bound { account });
                    Ok(true)
                }
                Some(WslLaunchBinding::Bound { account: current }) if *current == account => {
                    Ok(false)
                }
                _ => Err(Error::new(
                    ErrorCode::Conflict,
                    "This tab cannot change its WSL launch account.",
                )),
            }
        })?;
        Ok(())
    }

    /// Call only after the runtime confirms the original owned process tree is gone.
    pub fn terminal_stopped(&self, identity: TerminalIdentity) -> Result<()> {
        let mut inner = self.lock()?;
        match inner.registry.live.get(&identity.tab_id) {
            Some(generation) if *generation == identity.generation => {
                inner.registry.live.remove(&identity.tab_id);
                Ok(())
            }
            _ => Err(Error::new(
                ErrorCode::Conflict,
                "Terminal generation is no longer current.",
            )),
        }
    }
    pub fn prepare_close_tab(&self, id: TabId) -> Result<PreparedClose> {
        self.prepare_close(CloseTarget::Tab(id), None)
    }
    /// Compare the displayed generation and reserve cleanup under the same lock.
    pub fn prepare_close_terminal(&self, identity: TerminalIdentity) -> Result<PreparedClose> {
        self.prepare_close(CloseTarget::Tab(identity.tab_id), Some(identity))
    }
    pub fn prepare_delete_session(&self, id: SessionId) -> Result<PreparedClose> {
        self.prepare_close(CloseTarget::Session(id), None)
    }
    fn prepare_close(
        &self,
        target: CloseTarget,
        expected: Option<TerminalIdentity>,
    ) -> Result<PreparedClose> {
        let mut inner = self.lock()?;
        let data = storage::read(&inner.db)?;
        let (tabs, session) = match target {
            CloseTarget::Tab(id) => (vec![id], data.tab(id)?.session_id),
            CloseTarget::Session(id) => (data.session(id)?.tabs.iter().map(|t| t.id).collect(), id),
        };
        ensure_not_prepared(&inner.registry, &tabs, Some(session))?;
        let terminals = tabs
            .iter()
            .filter_map(|tab_id| {
                inner
                    .registry
                    .live
                    .get(tab_id)
                    .map(|generation| TerminalIdentity {
                        tab_id: *tab_id,
                        generation: *generation,
                    })
            })
            .collect::<Vec<_>>();
        if expected.is_some_and(|expected| terminals.as_slice() != [expected]) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Terminal generation is no longer current.",
            ));
        }
        let token = CleanupToken::new();
        inner.registry.pending.insert(
            token,
            PendingClose {
                target,
                revision: data.state.revision,
                terminals: terminals.clone(),
            },
        );
        Ok(PreparedClose { token, terminals })
    }
    /// Commit only after EVERY listed identity has completed owned cleanup.
    /// The token is an in-process trusted-adapter capability, never a remote RPC method.
    /// Failure preserves the token and registrations so callers can retry or cancel.
    pub fn commit_close(&self, token: CleanupToken) -> Result<Snapshot> {
        let mut inner = self.lock()?;
        let pending = inner
            .registry
            .pending
            .get(&token)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Cleanup token was not found."))?;
        let target = pending.target.clone();
        let revision = pending.revision;
        let terminals = pending.terminals.clone();
        let snapshot =
            self.update_locked(&mut inner, ChangeReason::Sessions, |data, registry| {
                if data.state.revision != revision {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Metadata changed during cleanup; prepare a new cleanup token.",
                    ));
                }
                let affected = match target {
                    CloseTarget::Tab(id) => vec![data.tab(id)?.id],
                    CloseTarget::Session(id) => {
                        data.session(id)?.tabs.iter().map(|t| t.id).collect()
                    }
                };
                for id in affected {
                    if let Some(generation) = registry.live.get(&id) {
                        if !terminals.contains(&TerminalIdentity {
                            tab_id: id,
                            generation: *generation,
                        }) {
                            return Err(Error::new(
                                ErrorCode::Conflict,
                                "Terminal ownership changed during cleanup.",
                            ));
                        }
                        return Err(Error::new(
                            ErrorCode::CleanupRequired,
                            "Owned cleanup must be confirmed before committing metadata deletion.",
                        ));
                    }
                }
                match target {
                    CloseTarget::Tab(id) => remove_tab(data, id)?,
                    CloseTarget::Session(id) => remove_session(data, id),
                }
                Ok(true)
            })?;
        inner.registry.pending.remove(&token);
        Ok(snapshot)
    }
    pub fn cancel_close(&self, token: CleanupToken) -> Result<()> {
        let mut inner = self.lock()?;
        inner
            .registry
            .pending
            .remove(&token)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Cleanup token was not found."))?;
        Ok(())
    }
}
