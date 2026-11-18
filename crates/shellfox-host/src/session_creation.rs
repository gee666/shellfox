use crate::{now, storage, *};

/// Durable metadata receipt only. This identifies no tab, generation, process,
/// launch success, or current selection. Missing/deleted receipts stay unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreationRequestReceipt {
    SessionCreated { session_id: SessionId },
    ReceiptUnavailable,
}
impl Host {
    /// Read the original request->session record, never Snapshot.selection.
    /// Unavailable cannot distinguish never-recorded from deleted: do not infer
    /// a replacement target or blindly recreate a known external request.
    pub fn creation_request_receipt(
        &self,
        request_id: SessionId,
    ) -> Result<CreationRequestReceipt> {
        let inner = self.lock()?;
        let data = storage::read(&inner.db)?;
        Ok(
            match data.requests.iter().find(|(id, _)| *id == request_id) {
                Some((_, session_id)) => CreationRequestReceipt::SessionCreated {
                    session_id: *session_id,
                },
                None => CreationRequestReceipt::ReceiptUnavailable,
            },
        )
    }

    /// Pure metadata creation deliberately has no verified adapter/shell facts.
    pub fn create_session(&self, input: CreateSession) -> Result<Snapshot> {
        self.create_session_intent(input, None)
    }
    /// Trusted Service adapter only: facts must come from the selected catalog
    /// profile BEFORE intent persistence, never from a client-supplied fact DTO.
    /// This records intent only and does not launch/adopt/identify any process.
    pub fn create_session_with_facts(
        &self,
        input: CreateSession,
        facts: SessionCreationFacts,
    ) -> Result<Snapshot> {
        facts.validate()?;
        self.create_session_intent(input, Some(facts))
    }
    fn create_session_intent(
        &self,
        input: CreateSession,
        facts: Option<SessionCreationFacts>,
    ) -> Result<Snapshot> {
        validation::cwd(&input.cwd)?;
        if let Some(title) = &input.title {
            validation::title(title)?;
        }
        let mut inner = self.lock()?;
        let runtime = inner.settings_runtime.clone();
        let tx = inner
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage::storage_error)?;
        let mut data = storage::load(&tx)?;
        crate::reserved_publication::ensure_request_available(&tx, input.request_id)?;
        // Idempotent retries preserve the ORIGINAL intent and fact provenance.
        // They may neither replace known facts nor backfill an unknown row.
        if !data.requests.iter().any(|(id, _)| *id == input.request_id) {
            let id = SessionId::new();
            let tab_id = TabId::new();
            let time = now();
            let title = input
                .title
                .unwrap_or_else(|| crate::sessions::directory_title(&input.cwd));
            let tab = Tab {
                id: tab_id,
                session_id: id,
                title: input.profile_id.to_string(),
                cwd: input.cwd.clone(),
                ordinal: 0,
                created_at: Some(now()),
                profile_id: input.profile_id,
                state: TabState::Pending,
            };
            data.sessions.push(Session {
                id,
                title,
                cwd: input.cwd,
                created_at: time.clone(),
                updated_at: time,
                settled_at: None,
                pinned_at: None,
                env: vec![],
                tabs: vec![tab],
            });
            if let Some(facts) = facts {
                data.creation_facts.insert(id, facts);
            }
            data.requests.push((input.request_id, id));
            data.select(id, Some(tab_id));
            data.state.revision = data
                .state
                .revision
                .checked_add(1)
                .ok_or_else(|| storage::storage_error("revision overflow"))?;
            data.state.last_change = Some(ChangedEvent {
                revision: data.state.revision,
                reason: ChangeReason::Sessions,
                select_session_id: Some(id),
            });
            storage::save(&tx, &data)?;
        }
        let snapshot = data.snapshot();
        if let Some(runtime) = runtime {
            runtime.check_commit()?;
        }
        tx.commit().map_err(storage::storage_error)?;
        Ok(snapshot)
    }
}
