use crate::{
    assets, content, paths, staged_write, stream_io,
    transfer_registry::{Transfer, TransferRead, TransferState},
    *,
};
use std::{sync::Arc, time::Instant};
use uuid::Uuid;

impl TransferRegistry {
    pub fn begin_read(
        &mut self,
        actor: &TransferActor,
        capability: Arc<FileService>,
        input: &BeginRead,
        auth: &dyn TransferAuthorization,
    ) -> Result<ReadStarted> {
        self.begin_read_capped(actor, capability, input, auth, self.limits.max_file_bytes)
    }
    pub(crate) fn begin_read_capped(
        &mut self,
        actor: &TransferActor,
        capability: Arc<FileService>,
        input: &BeginRead,
        auth: &dyn TransferAuthorization,
        limit: u64,
    ) -> Result<ReadStarted> {
        self.check(actor, &capability, false, auth)?;
        self.reserve(actor, 0)?;
        let (target, asset_mime) = if let Some(asset_path) = &input.asset_path {
            let (target, mime) = assets::asset_target(
                &capability,
                &AssetRequest {
                    path: input.path.clone(),
                    asset_path: asset_path.clone(),
                },
            )?;
            (target, Some(mime))
        } else {
            (capability.target(&input.path)?, None)
        };
        let mut file = content::open_regular(&target.parent, &target.name)?;
        let check = || self.check(actor, &capability, false, auth);
        let mut inspected = stream_io::inspect(
            &mut file,
            &target.absolute,
            self.limits.max_file_bytes.min(limit),
            &check,
        )?;
        if let Some(mime) = asset_mime {
            inspected.mime_type = mime.to_owned();
        }
        let id = Uuid::new_v4();
        let result = ReadStarted {
            transfer_id: id,
            path: paths::display(&target.absolute)?,
            size: inspected.size,
            revision: inspected.revision.clone(),
            kind: inspected.kind,
            mime_type: inspected.mime_type.clone(),
            chunk_bytes: CHUNK_BYTES,
        };
        check()?;
        self.entries.insert(
            id,
            Transfer {
                actor: actor.clone(),
                capability,
                touched: Instant::now(),
                state: TransferState::Read(Box::new(TransferRead {
                    file,
                    inspected,
                    offset: 0,
                    last: None,
                })),
            },
        );
        Ok(result)
    }
    pub fn begin_write(
        &mut self,
        actor: &TransferActor,
        capability: Arc<FileService>,
        input: &BeginWrite,
        auth: &dyn TransferAuthorization,
    ) -> Result<WriteStarted> {
        // Before check/reserve: expiry can drop stages and persist their journal.
        staged_write::require_user_stage_contract(&capability)?;
        staged_write::validate(input, self.limits.max_file_bytes)?;
        self.check(actor, &capability, true, auth)?;
        self.reserve(actor, input.size)?;
        let check = || self.check(actor, &capability, true, auth);
        let stage = staged_write::StagedWrite::begin_with_journal(
            capability.clone(),
            input,
            self.limits.max_file_bytes,
            &check,
            self.journal.clone(),
        )?;
        let transfer_id = Uuid::new_v4();
        self.entries.insert(
            transfer_id,
            Transfer {
                actor: actor.clone(),
                capability,
                touched: Instant::now(),
                state: TransferState::Write(Box::new(stage)),
            },
        );
        Ok(WriteStarted {
            transfer_id,
            chunk_bytes: CHUNK_BYTES,
        })
    }
}
