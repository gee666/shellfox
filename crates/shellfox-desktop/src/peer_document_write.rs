use super::*;
use shellfox_client::FileWriteCursor;
impl<T: OutboundTransport> PeerDocumentBackend<T> {
    pub async fn write(
        &self,
        input: &WriteText,
        cancel: &EditorCancellation,
    ) -> Result<WriteResult> {
        self.check_cancel(cancel)?;
        wire::files::path(&input.path).map_err(|_| anyhow!("Invalid peer document path."))?;
        wire::files::revision(&input.revision)
            .map_err(|_| anyhow!("Invalid peer document revision."))?;
        ensure!(
            input.text.len() <= shellfox_files::TRANSFER_TEXT_BYTES as usize
                && !input.text.contains('\0'),
            "Invalid peer text or8MiB editor limit exceeded."
        );
        let attempt = self.begin_save(&input.path)?;
        let request = wire::BeginFileWrite {
            session_id: None,
            tab_id: None,
            path: input.path.clone(),
            size: input.text.len() as u64,
            mode: wire::FileWriteMode::Replace,
            revision: Some(input.revision.clone()),
            content: wire::FileContent::Text,
            mime_type: None,
        };
        // Begin is issued ONCE. A missing response cannot be replayed or adopted by a new binding.
        let started = self.execute(&m::BeginFileWrite(request.clone())).await?;
        let mut cursor = FileWriteCursor::new(request, started)
            .map_err(|e| anyhow!("Invalid peer write transfer: {e:?}"))?;
        let mut finishing = false;
        let result = self
            .write_chunks(input, &mut cursor, &mut finishing, cancel)
            .await;
        if result.is_err() && !finishing {
            self.cancel_once(cursor.cancel_request()).await;
        }
        let written = result?;
        self.check()?;
        attempt.confirm()?;
        self.check()?;
        Ok(written)
    }
    async fn write_chunks(
        &self,
        input: &WriteText,
        cursor: &mut FileWriteCursor,
        finishing: &mut bool,
        cancel: &EditorCancellation,
    ) -> Result<WriteResult> {
        for bytes in input.text.as_bytes().chunks(wire::FILE_CHUNK_BYTES) {
            self.check_cancel(cancel)?;
            let request = cursor
                .write(bytes)
                .map_err(|e| anyhow!("Peer write transfer refused: {e:?}"))?;
            let reply = self.execute(&m::WriteFileChunk(request)).await?;
            self.check_cancel(cancel)?;
            cursor
                .written(&reply)
                .map_err(|e| anyhow!("Invalid peer write acknowledgement: {e:?}"))?;
        }
        self.check_cancel(cancel)?;
        let request = cursor
            .finish()
            .map_err(|e| anyhow!("Peer finish refused: {e:?}"))?;
        *finishing = true;
        // From here, no cancel/retry/rollback after a missing, invalid or stale response.
        let reply = self.execute(&m::FinishFileWrite(request)).await?;
        cursor.finished(&reply).map_err(|e| {
            anyhow!("Unconfirmed peer publication: {e:?}. Reload before saving again.")
        })?;
        ensure!(
            reply.path == input.path,
            "Peer save result names another target. Outcome unconfirmed; reload before saving again."
        );
        self.check()?;
        Ok(WriteResult {
            path: reply.path,
            size: reply.size,
            revision: reply.revision,
        })
    }
}
