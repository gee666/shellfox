use super::*;
use shellfox_client::FileReadCursor;

impl<T: OutboundTransport> PeerDocumentBackend<T> {
    pub async fn read(&self, path: &str, cancel: &EditorCancellation) -> Result<ReadResult> {
        let mut cursor = self.begin_read(path, None, cancel).await?;
        let result = self.read_preview(&mut cursor, cancel).await;
        self.cancel_once(cursor.cancel_request()).await;
        let result = result?;
        let canonical = match &result {
            ReadResult::Text { path, .. }
            | ReadResult::Image { path, .. }
            | ReadResult::Binary { path, .. } => path,
        };
        self.check_cancel(cancel)?;
        self.read_confirmed(path, canonical)?;
        self.check_cancel(cancel)?;
        Ok(result)
    }

    pub async fn read_asset(
        &self,
        path: &str,
        asset: String,
        cancel: &EditorCancellation,
    ) -> Result<shellfox_files::FileAsset> {
        let mut cursor = self.begin_read(path, Some(asset), cancel).await?;
        let header = cursor.header().clone();
        let result = async {
            ensure!(
                header.size <= shellfox_files::TRANSFER_IMAGE_BYTES,
                "Image and asset preview is limited to 64 MiB. Use Download for larger files."
            );
            let mut bytes = Vec::with_capacity(header.size as usize);
            self.receive_chunks(&mut cursor, cancel, |data, _| {
                bytes.extend_from_slice(data);
                Ok(())
            })
            .await?;
            ensure!(
                bytes.len() as u64 == header.size,
                "Peer asset length changed."
            );
            Ok(shellfox_files::FileAsset {
                mime_type: header.mime_type,
                size: header.size,
                bytes,
            })
        }
        .await;
        self.close_read(&cursor).await;
        result
    }

    pub async fn begin_read(
        &self,
        path: &str,
        asset_path: Option<String>,
        cancel: &EditorCancellation,
    ) -> Result<FileReadCursor> {
        self.check_cancel(cancel)?;
        let (session_id, tab_id) = self.ids();
        let request = wire::BeginFileRead {
            session_id,
            tab_id,
            path: path.into(),
            asset_path,
        };
        let header = self.execute(&request).await?;
        FileReadCursor::new(header).map_err(|e| anyhow!("Invalid peer read transfer: {e:?}"))
    }

    pub async fn close_read(&self, cursor: &FileReadCursor) {
        self.cancel_once(cursor.cancel_request()).await;
    }

    pub async fn receive_chunks(
        &self,
        cursor: &mut FileReadCursor,
        cancel: &EditorCancellation,
        mut consume: impl FnMut(&[u8], u64) -> Result<()>,
    ) -> Result<()> {
        while !cursor.is_complete() {
            self.check_cancel(cancel)?;
            let request = cursor
                .request()
                .map_err(|e| anyhow!("Peer read transfer refused: {e:?}"))?;
            let chunk = self.execute(&request).await?;
            self.check_cancel(cancel)?;
            let data = cursor
                .receive(&chunk)
                .map_err(|e| anyhow!("Invalid peer file chunk: {e:?}"))?;
            consume(&data, chunk.offset)?;
            cursor
                .consumed()
                .map_err(|e| anyhow!("Peer read transfer refused: {e:?}"))?;
        }
        self.check_cancel(cancel)
    }

    async fn read_preview(
        &self,
        cursor: &mut FileReadCursor,
        cancel: &EditorCancellation,
    ) -> Result<ReadResult> {
        let header = cursor.header().clone();
        let wire::FileReadTransfer {
            path,
            mime_type,
            size,
            revision,
            kind,
            ..
        } = header;
        // The authenticated host resolves links and returns the canonical path.
        let (limit, error) = match kind {
            wire::FileReadKind::Binary => {
                return Ok(ReadResult::Binary {
                    path,
                    mime_type,
                    size,
                    revision,
                });
            }
            wire::FileReadKind::Text => (
                shellfox_files::TRANSFER_TEXT_BYTES,
                "Text preview is limited to 8 MiB. Use Download for larger files.",
            ),
            wire::FileReadKind::Image => (
                shellfox_files::TRANSFER_IMAGE_BYTES,
                "Image and asset preview is limited to 64 MiB. Use Download for larger files.",
            ),
        };
        ensure!(size <= limit, "{error}");
        let mut bytes = Vec::with_capacity(size as usize);
        self.receive_chunks(cursor, cancel, |data, _| {
            bytes.extend_from_slice(data);
            Ok(())
        })
        .await?;
        ensure!(bytes.len() as u64 == size, "Peer document length changed.");
        if kind == wire::FileReadKind::Image {
            return Ok(ReadResult::Image {
                path,
                mime_type,
                size,
                revision,
                bytes,
            });
        }
        let text =
            String::from_utf8(bytes).map_err(|_| anyhow!("Peer document is not UTF-8 text."))?;
        ensure!(!text.contains('\0'), "Peer document contains NUL.");
        Ok(ReadResult::Text {
            path,
            mime_type,
            size,
            revision,
            text,
        })
    }
}
