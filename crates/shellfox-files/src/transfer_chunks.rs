use crate::{
    stream_io,
    transfer_registry::{Receipt, TransferRead, TransferState},
    *,
};
use std::time::Instant;

impl TransferRegistry {
    pub fn read_chunk(
        &mut self,
        actor: &TransferActor,
        input: &ReadChunk,
        auth: &dyn TransferAuthorization,
    ) -> Result<ReadChunkResult> {
        let mut entry = self.take(actor, input.transfer_id)?;
        self.check(actor, &entry.capability, false, auth)?;
        let check = || self.check(actor, &entry.capability, false, auth);
        let TransferState::Read(reader) = &mut entry.state else {
            return Err(FileError::new(
                ErrorCode::Validation,
                "This is a write transfer.",
            ));
        };
        let TransferRead {
            file,
            inspected,
            offset,
            last,
            ..
        } = reader.as_mut();
        let replay = last
            .as_ref()
            .filter(|(previous, _)| *previous == input.offset);
        let result = if let Some((_, result)) = replay {
            // Replays remain bound to the same opened source version, not cached authorization.
            if !crate::content::same_version(&inspected.snapshot, &crate::content::snapshot(file)?)
            {
                return Err(FileError::conflict());
            }
            check()?;
            result.clone()
        } else {
            if input.offset != *offset {
                return Err(FileError::new(
                    ErrorCode::Validation,
                    "Unexpected download offset.",
                ));
            }
            let data = stream_io::checked_read(file, &inspected.snapshot, *offset, &check)?;
            *offset += data.len() as u64;
            ReadChunkResult {
                offset: *offset,
                eof: *offset == inspected.size,
                data,
            }
        };
        *last = Some((input.offset, result.clone()));
        entry.touched = Instant::now();
        self.entries.insert(input.transfer_id, entry);
        Ok(result)
    }
    pub fn write_chunk(
        &mut self,
        actor: &TransferActor,
        input: &WriteChunk,
        auth: &dyn TransferAuthorization,
    ) -> Result<TransferProgress> {
        let mut entry = self.take(actor, input.transfer_id)?;
        self.check(actor, &entry.capability, true, auth)?;
        let check = || self.check(actor, &entry.capability, true, auth);
        let TransferState::Write(stage) = &mut entry.state else {
            return Err(FileError::new(
                ErrorCode::Validation,
                "This is a read transfer.",
            ));
        };
        let result = stage.write(&entry.capability, input, &check)?;
        entry.touched = Instant::now();
        self.entries.insert(input.transfer_id, entry);
        Ok(result)
    }
    /// Completion retries return a bounded cached receipt, never repeat filesystem publication.
    pub fn finish_write(
        &mut self,
        actor: &TransferActor,
        input: &TransferIdentity,
        auth: &dyn TransferAuthorization,
    ) -> Result<WriteResult> {
        if let Some(receipt) = self
            .receipts
            .iter()
            .find(|receipt| receipt.id == input.transfer_id && &receipt.actor == actor)
        {
            self.check(actor, &receipt.capability, true, auth)?;
            if receipt.touched.elapsed() > self.limits.idle_ttl {
                return Err(crate::transfer_registry::missing());
            }
            return Ok(receipt.result.clone());
        }
        let mut entry = self.take(actor, input.transfer_id)?;
        self.check(actor, &entry.capability, true, auth)?;
        let check = || self.check(actor, &entry.capability, true, auth);
        let TransferState::Write(stage) = &mut entry.state else {
            return Err(FileError::new(
                ErrorCode::Validation,
                "This is a read transfer.",
            ));
        };
        // Capture the actual stage identity before publication, not a fallible post-effect query.
        let published = crate::HandleIdentity::from_cap_file(&stage.file)?;
        let result = stage.commit(&entry.capability, self.limits.max_file_bytes, &check)?;
        self.receipts.push_back(Receipt {
            id: input.transfer_id,
            actor: actor.clone(),
            capability: entry.capability.clone(),
            touched: Instant::now(),
            result: result.clone(),
            published,
        });
        while self.receipts.len() > TRANSFER_RECEIPTS {
            self.receipts.pop_front();
        }
        Ok(result)
    }
}
