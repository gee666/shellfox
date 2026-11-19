use super::*;
use shellfox_client::FileWriteCursor;
use std::io::{Cursor, Read};

impl<T: OutboundTransport> PeerDocumentBackend<T> {
    pub async fn write(
        &self,
        input: &WriteText,
        cancel: &EditorCancellation,
    ) -> Result<WriteResult> {
        ensure!(
            input.text.len() as u64 <= shellfox_files::TRANSFER_TEXT_BYTES
                && !input.text.contains('\0'),
            "Invalid peer text or 8 MiB editor limit exceeded."
        );
        self.replace(
            &input.path,
            &input.revision,
            input.text.as_bytes(),
            None,
            cancel,
        )
        .await
    }

    pub async fn write_image(
        &self,
        input: &ImageWrite,
        cancel: &EditorCancellation,
    ) -> Result<WriteResult> {
        ensure!(
            !input.bytes.is_empty()
                && input.bytes.len() as u64 <= shellfox_files::TRANSFER_IMAGE_BYTES,
            "Invalid peer image or 64 MiB editor limit exceeded."
        );
        let mime = match input.mime_type.as_str() {
            "image/png" => wire::ImageWriteMime::Png,
            "image/jpeg" => wire::ImageWriteMime::Jpeg,
            "image/webp" => wire::ImageWriteMime::Webp,
            _ => anyhow::bail!("Only PNG, JPEG and WebP image saves are supported."),
        };
        self.replace(
            &input.path,
            &input.revision,
            &input.bytes,
            Some(mime),
            cancel,
        )
        .await
    }

    async fn replace(
        &self,
        path: &str,
        revision: &str,
        bytes: &[u8],
        mime_type: Option<wire::ImageWriteMime>,
        cancel: &EditorCancellation,
    ) -> Result<WriteResult> {
        self.check_cancel(cancel)?;
        let attempt = self.begin_save(path)?;
        let request = wire::BeginFileWrite {
            session_id: None,
            tab_id: None,
            path: path.into(),
            size: bytes.len() as u64,
            mode: wire::FileWriteMode::Replace,
            revision: Some(revision.into()),
            content: if mime_type.is_some() {
                wire::FileContent::Image
            } else {
                wire::FileContent::Text
            },
            mime_type,
        };
        let result = self
            .send_chunks(request, Cursor::new(bytes), cancel, |_| Ok(()))
            .await;
        if result.is_ok()
            || result.as_ref().err().is_some_and(|error| {
                error
                    .downcast_ref::<shellfox_files::FileError>()
                    .is_some_and(|error| error.code == shellfox_files::ErrorCode::FileConflict)
            })
        {
            attempt.confirm()?;
        }
        self.check()?;
        result
    }

    pub async fn send_chunks(
        &self,
        request: wire::BeginFileWrite,
        mut input: impl Read,
        cancel: &EditorCancellation,
        progress: impl FnMut(u64) -> Result<()>,
    ) -> Result<WriteResult> {
        self.check_cancel(cancel)?;
        let started = self.execute(&request).await?;
        let path = request.path.clone();
        let size = request.size;
        let mut cursor = FileWriteCursor::new(request, started)
            .map_err(|e| anyhow!("Invalid peer write transfer: {e:?}"))?;
        let mut finishing = false;
        let result = self
            .transmit(
                &path,
                size,
                &mut input,
                &mut cursor,
                &mut finishing,
                cancel,
                progress,
            )
            .await;
        if result.is_err() && !finishing {
            self.cancel_once(cursor.cancel_request()).await;
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn transmit(
        &self,
        path: &str,
        size: u64,
        input: &mut impl Read,
        cursor: &mut FileWriteCursor,
        finishing: &mut bool,
        cancel: &EditorCancellation,
        mut progress: impl FnMut(u64) -> Result<()>,
    ) -> Result<WriteResult> {
        let mut offset = 0;
        let mut buf = vec![0; wire::FILE_CHUNK_BYTES];
        while offset < size {
            self.check_cancel(cancel)?;
            let length = (size - offset).min(buf.len() as u64) as usize;
            input.read_exact(&mut buf[..length])?;
            let request = cursor
                .write(&buf[..length])
                .map_err(|e| anyhow!("Peer write transfer refused: {e:?}"))?;
            let reply = self.execute(&request).await?;
            self.check_cancel(cancel)?;
            cursor
                .written(&reply)
                .map_err(|e| anyhow!("Invalid peer write acknowledgement: {e:?}"))?;
            offset = reply.offset;
            progress(offset)?;
        }
        self.check_cancel(cancel)?;
        let request = cursor
            .finish()
            .map_err(|e| anyhow!("Peer finish refused: {e:?}"))?;
        *finishing = true;
        // Do not retry or roll back once publication may have started.
        let reply = self.execute(&request).await?;
        cursor.finished(&reply).map_err(|e| {
            anyhow!("Unconfirmed peer publication: {e:?}. Reload before saving again.")
        })?;
        ensure!(
            reply.path == path,
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
