use crate::{
    paths,
    upload_inbox::{PendingUpload, SavedUpload},
    *,
};
use std::time::{Instant, SystemTime};
use uuid::Uuid;

impl UploadInbox {
    pub fn begin(
        &mut self,
        actor: &UploadActor,
        input: &BeginUpload,
        auth: &dyn UploadAuthorization,
    ) -> Result<UploadStarted> {
        self.authorized(actor, auth)?;
        let check = upload_check(actor, auth);
        self.registry
            .check(&actor.connection, &self.capability, true, &check)?;
        if input.file_name.is_empty()
            || input.file_name.encode_utf16().count() > 1024
            || input.size > self.limits.max_file_bytes
        {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Invalid upload file name or size.",
            ));
        }
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, entry)| entry.touched.elapsed() > self.limits.active_ttl)
            .map(|(id, entry)| (*id, entry.clone()))
            .collect();
        for (id, entry) in expired {
            self.abort(id, &entry)?;
        }
        self.expire_saved()?;
        let usage = self.usage();
        let total = (512 * 1024 * 1024).max(self.limits.max_file_bytes * 2);
        if usage.saved + usage.active + self.preserved_count >= UPLOAD_ENTRIES
            || usage
                .reserved_bytes
                .saturating_add(self.preserved_bytes)
                .saturating_add(input.size)
                > total
        {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Host upload inbox quota reached.",
            ));
        }
        let id = Uuid::new_v4();
        let directory = id.to_string();
        let file_name = sanitize_upload_name(&input.file_name);
        let created = self.create_upload_directory(&directory)?;
        let result = (|| {
            let check = upload_check(actor, auth);
            let started = self.registry.begin_write(
                &actor.connection,
                self.capability.clone(),
                &BeginWrite {
                    path: format!("{directory}/{file_name}"),
                    size: input.size,
                    mode: WriteMode::Create,
                    revision: None,
                    content: TransferContent::File,
                    mime_type: None,
                },
                &check,
            )?;
            Ok(started)
        })();
        let started = match result {
            Ok(started) => started,
            Err(error) => {
                // The stage may exist; leave the directory for the abort path.
                drop(created);
                return Err(error);
            }
        };
        drop(created); // the stage is open; the directory no longer needs pinning
        self.pending.insert(
            id,
            PendingUpload {
                actor: actor.clone(),
                transfer_id: started.transfer_id,
                file_name,
                size: input.size,
                touched: Instant::now(),
            },
        );
        Ok(UploadStarted {
            upload_id: id,
            chunk_bytes: CHUNK_BYTES,
        })
    }
    pub fn write_chunk(
        &mut self,
        actor: &UploadActor,
        input: &UploadChunk,
        auth: &dyn UploadAuthorization,
    ) -> Result<TransferProgress> {
        let entry = self.selected(actor, input.upload_id)?;
        let outcome = (|| {
            self.authorized(actor, auth)?;
            self.registry.write_chunk(
                &actor.connection,
                &WriteChunk {
                    transfer_id: entry.transfer_id,
                    offset: input.offset,
                    data: input.data.clone(),
                },
                &upload_check(actor, auth),
            )
        })();
        match outcome {
            Ok(progress) => {
                if let Some(pending) = self.pending.get_mut(&input.upload_id) {
                    pending.touched = Instant::now();
                }
                Ok(progress)
            }
            Err(error) => {
                self.abort(input.upload_id, &entry)?;
                Err(error)
            }
        }
    }
    pub fn finish(
        &mut self,
        actor: &UploadActor,
        input: &UploadIdentity,
        auth: &dyn UploadAuthorization,
        resolver: &dyn UploadPathResolver,
    ) -> Result<UploadFinished> {
        if let Some(saved) = self.saved.get(&input.upload_id) {
            self.authorized(actor, auth)?;
            let (owner, result) = &saved.completion;
            if owner == actor {
                return Ok(result.clone());
            }
            return Err(crate::transfer_registry::missing());
        }
        let entry = self.selected(actor, input.upload_id)?;
        let outcome = (|| {
            self.authorized(actor, auth)?;
            self.finish_selected(actor, input.upload_id, &entry, auth, resolver)
        })();
        if outcome.is_err() {
            if let Some(saved) = self.saved.get(&input.upload_id) {
                self.remove_saved(input.upload_id, saved)?;
                self.saved.remove(&input.upload_id);
            }
            self.abort(input.upload_id, &entry)?;
        }
        outcome
    }
    fn finish_selected(
        &mut self,
        actor: &UploadActor,
        id: Uuid,
        entry: &PendingUpload,
        auth: &dyn UploadAuthorization,
        resolver: &dyn UploadPathResolver,
    ) -> Result<UploadFinished> {
        let relative = format!("{id}/{}", entry.file_name);
        let native = self.root.join(&relative);
        let resolved = resolver.resolve(&native, actor)?;
        paths::validate(&resolved)?;
        match &actor.environment {
            UploadEnvironment::Local if resolved != paths::display(&native)? => {
                return Err(FileError::denied())
            }
            UploadEnvironment::Wsl { .. }
                if !resolved.starts_with('/') || resolved.contains('\\') =>
            {
                return Err(FileError::denied())
            }
            _ => {}
        }
        // Translation can block; never use its pre-translation permission snapshot.
        self.authorized(actor, auth)?;
        let result = self.registry.finish_write(
            &actor.connection,
            &TransferIdentity {
                transfer_id: entry.transfer_id,
            },
            &upload_check(actor, auth),
        )?;
        let identity = self
            .registry
            .receipts
            .iter()
            .find(|receipt| receipt.id == entry.transfer_id)
            .map(|receipt| receipt.published.clone())
            .ok_or_else(|| {
                FileError::new(
                    ErrorCode::StorageFailed,
                    "Published upload evidence is unavailable. Do not retry automatically.",
                )
            })?;
        let finished = UploadFinished {
            path: resolved,
            file_name: entry.file_name.clone(),
            size: result.size,
            revision: result.revision,
        };
        self.saved.insert(
            id,
            SavedUpload {
                relative,
                size: result.size,
                touched: SystemTime::now(),
                identity,
                completion: (actor.clone(), finished.clone()),
            },
        );
        self.authorized(actor, auth)?;
        self.pending.remove(&id);
        Ok(finished)
    }
    fn abort(&mut self, id: Uuid, entry: &PendingUpload) -> Result<()> {
        let _ = self.registry.cancel(
            &entry.actor.connection,
            &TransferIdentity {
                transfer_id: entry.transfer_id,
            },
        );
        self.pending.remove(&id);
        self.remove_empty_directory(id)
    }
}
fn upload_check<'a>(
    actor: &'a UploadActor,
    auth: &'a dyn UploadAuthorization,
) -> impl TransferAuthorization + 'a {
    move |connection: &TransferActor, capability: &FileService, write: bool| {
        if connection != &actor.connection
            || !write
            || capability.capability_kind() != CapabilityKind::ApprovedRoot
        {
            return Err(FileError::denied());
        }
        auth.check(actor)
    }
}
