use super::*;
impl ApprovalControl {
    pub(crate) async fn authorize_pair_async(
        &self,
        peer: PeerIdentity,
        client: ClientId,
        now: u64,
    ) -> Result<Actor, AppError> {
        // A durable change temporarily closes the policy. Pair must not treat
        // that fence as a genuine denial of this attempt.
        let _commit = self.management.lock().await;
        self.authorize_async(peer, client, now).await
    }

    pub async fn load(store: &PrivateApprovalStore) -> Result<Self, AppError> {
        let loaded = store.load_policy().await?;
        if loaded.migrated {
            return Err(native_unavailable());
        }
        let control = Self::default();
        for approval in loaded.approvals {
            control.install_committed(approval)?;
        }
        Ok(control)
    }
    fn install_committed(&self, approval: Approval) -> Result<(), AppError> {
        let approval = crate::approval_records::normalized(approval)?;
        let files = vec![Arc::new(FileService::approved_full_account())];
        let mut state = self.lock()?;
        if state.native_unavailable {
            return Err(native_unavailable());
        }
        state.epoch = state.epoch.checked_add(1).ok_or_else(denied)?;
        let epoch = state.epoch;
        state.pending.remove(&approval.peer_id);
        state.grants.insert(
            approval.peer_id.clone(),
            Grant {
                approval,
                epoch,
                files,
            },
        );
        self.notify();
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn install(&self, approval: Approval) -> Result<(), AppError> {
        self.install_committed(approval)
    }
    /// Permanently close this policy instance's native persistence boundary.
    /// Denial happens before capability revocation or later view retirement.
    fn deny_native_runtime(&self) -> AppError {
        let result = (|| {
            let mut state = self.lock()?;
            state.enabled = false;
            state.native_unavailable = true;
            let grants = std::mem::take(&mut state.grants);
            state.pending.clear();
            let mut revocation_failed = false;
            for grant in grants.into_values() {
                for files in grant.files {
                    if files.revoke().is_err() {
                        revocation_failed = true;
                    }
                }
            }
            self.notify();
            if revocation_failed {
                return Err(error(
                    ErrorCode::NativeUnavailable,
                    "Native authority is denied; capability cleanup remains unavailable.",
                    false,
                ));
            }
            Ok(())
        })();
        result.err().unwrap_or_else(native_unavailable)
    }
    pub async fn approve_full_account(
        &self,
        store: &PrivateApprovalStore,
        peer_id: &str,
    ) -> Result<(), AppError> {
        let peer = {
            let state = self.lock()?;
            state.pending.get(peer_id).cloned().ok_or_else(denied)?
        };
        self.approve(
            store,
            Approval {
                peer_id: peer.id,
                hostname: peer.hostname,
                approved_at_millis: crate::backend::now_millis(),
                permission: ViewPermission::Write,
                file_roots: Vec::new(),
            },
        )
        .await
    }
    fn prepare_change(
        &self,
        peer_id: &str,
        approval: Option<Approval>,
    ) -> Result<(Vec<Approval>, u64), AppError> {
        let mut state = self.lock()?;
        if state.native_unavailable {
            return Err(native_unavailable());
        }
        // Deny before capability revocation, enqueue or any persistence I/O.
        // Cancellation keeps this fence closed until a new policy is loaded.
        state.native_unavailable = true;
        self.notify();
        if let Some(old) = state.grants.remove(peer_id) {
            for files in old.files {
                files.revoke().map_err(|_| denied())?;
            }
        }
        state.pending.remove(peer_id);
        if approval.is_none() {
            deny_attempts(&mut state, peer_id);
        }
        state.epoch = state.epoch.checked_add(1).ok_or_else(denied)?;
        let epoch = state.epoch;
        let mut records: Vec<_> = state
            .grants
            .values()
            .map(|grant| grant.approval.clone())
            .collect();
        if let Some(approval) = approval {
            records.push(approval);
        }
        Ok((records, epoch))
    }
    async fn change(
        &self,
        store: &PrivateApprovalStore,
        peer_id: String,
        approval: Option<Approval>,
    ) -> Result<(), AppError> {
        let _management = self.management.lock().await;
        let control = self.clone();
        let candidate = approval.clone();
        let target = peer_id.clone();
        let (records, epoch) =
            crate::approval_async::blocking(move || control.prepare_change(&target, candidate))
                .await?;
        if let Err(error) = store.save(&records).await {
            let control = self.clone();
            let _ = crate::approval_async::blocking(move || {
                control.deny_native_runtime();
                Ok(())
            })
            .await;
            return Err(error);
        }
        let control = self.clone();
        crate::approval_async::blocking(move || {
            let mut state = control.lock()?;
            if !state.native_unavailable || state.epoch != epoch {
                return Err(native_unavailable());
            }
            if let Some(approval) = approval {
                let files = vec![Arc::new(FileService::approved_full_account())];
                state.grants.insert(
                    peer_id,
                    Grant {
                        approval,
                        epoch,
                        files,
                    },
                );
            }
            // No await between checking the confirmed commit and installing.
            state.native_unavailable = false;
            control.notify();
            Ok(())
        })
        .await
    }
    pub async fn approve(
        &self,
        store: &PrivateApprovalStore,
        approval: Approval,
    ) -> Result<(), AppError> {
        let approval = crate::approval_records::normalized(approval)?;
        self.change(store, approval.peer_id.clone(), Some(approval))
            .await
    }
    pub async fn revoke(
        &self,
        store: &PrivateApprovalStore,
        peer_id: &str,
    ) -> Result<(), AppError> {
        self.change(store, peer_id.to_owned(), None).await
    }
    #[cfg(test)]
    pub(crate) fn reject_inner(&self, peer_id: &str, tombstone: bool) -> Result<(), AppError> {
        let mut state = self.lock()?;
        if let Some(grant) = state.grants.remove(peer_id) {
            for file in grant.files {
                file.revoke().map_err(|_| denied())?;
            }
        }
        state.pending.remove(peer_id);
        if tombstone {
            deny_attempts(&mut state, peer_id);
        }
        self.notify();
        Ok(())
    }
    pub fn reject(&self, peer_id: &str) -> Result<(), AppError> {
        let mut state = self.lock()?;
        if state.grants.contains_key(peer_id) {
            return Err(error(
                ErrorCode::Validation,
                "Persisted native approval management is unavailable.",
                false,
            ));
        }
        deny_attempts(&mut state, peer_id);
        self.notify();
        Ok(())
    }
}
fn native_unavailable() -> AppError {
    error(ErrorCode::Unsupported,"Native persisted approval authority is unavailable pending approval/revocation storage review.",false)
}
fn deny_attempts(state: &mut State, peer_id: &str) {
    state.pending.remove(peer_id);
    if let Some(attempts) = state.attempts.get_mut(peer_id) {
        let now = crate::backend::now_millis();
        for (client, _) in attempts.seen.drain() {
            attempts.denied.insert(client, now + DENIED_TTL);
        }
    }
}
