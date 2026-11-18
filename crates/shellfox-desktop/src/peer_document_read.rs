use super::*;
use shellfox_client::FileReadCursor;
impl<T: OutboundTransport> PeerDocumentBackend<T> {
    pub async fn read(&self, path: &str, cancel: &EditorCancellation) -> Result<TextFile> {
        self.check_cancel(cancel)?;
        wire::files::path(path).map_err(|_| anyhow!("Invalid peer document path."))?;
        let header = self
            .execute(&m::BeginFileRead(wire::BeginFileRead {
                session_id: None,
                tab_id: None,
                path: path.into(),
                asset_path: None,
            }))
            .await?;
        let mut cursor = FileReadCursor::new(header)
            .map_err(|e| anyhow!("Invalid peer read transfer: {e:?}"))?;
        let result = self.read_chunks(path, &mut cursor, cancel).await;
        // Read transfer disposal is not a filesystem-delete authority. No reconnect/retry.
        self.cancel_once(cursor.cancel_request()).await;
        let text = result?;
        self.check_cancel(cancel)?;
        // This is denial bookkeeping, never authority. Do not block on its mutex under
        // the Manager/SDK origin fence; the ORIGINAL admission is checked again afterward.
        self.read_confirmed(path, &text.path)?;
        self.check_cancel(cancel)?;
        Ok(text)
    }
    async fn read_chunks(
        &self,
        expected: &str,
        cursor: &mut FileReadCursor,
        cancel: &EditorCancellation,
    ) -> Result<TextFile> {
        let header = cursor.header().clone();
        ensure!(
            header.path == expected,
            "Peer read response names another target."
        );
        ensure!(
            header.kind == wire::FileReadKind::Text,
            "This peer file is not editable text."
        );
        ensure!(
            header.size <= shellfox_files::TRANSFER_TEXT_BYTES,
            "Peer document exceeds the8MiB editor limit."
        );
        let mut bytes = Vec::with_capacity(header.size as usize);
        while !cursor.is_complete() {
            self.check_cancel(cancel)?;
            let request = cursor
                .request()
                .map_err(|e| anyhow!("Peer read transfer refused: {e:?}"))?;
            let chunk = self.execute(&m::ReadFileChunk(request)).await?;
            self.check_cancel(cancel)?;
            let data = cursor
                .receive(&chunk)
                .map_err(|e| anyhow!("Invalid peer file chunk: {e:?}"))?;
            ensure!(
                bytes.len().saturating_add(data.len())
                    <= shellfox_files::TRANSFER_TEXT_BYTES as usize,
                "Peer document exceeded its bound."
            );
            bytes.extend_from_slice(&data);
            cursor
                .consumed()
                .map_err(|e| anyhow!("Peer read transfer refused: {e:?}"))?;
        }
        ensure!(
            bytes.len() as u64 == header.size,
            "Peer document length changed."
        );
        let text =
            String::from_utf8(bytes).map_err(|_| anyhow!("Peer document is not UTF-8 text."))?;
        ensure!(!text.contains('\0'), "Peer document contains NUL.");
        self.check_cancel(cancel)?;
        Ok(TextFile {
            path: header.path,
            text,
            size: header.size,
            revision: header.revision,
        })
    }
}
