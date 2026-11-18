use crate::{assets, staged_write::StagedWrite, *};
use uuid::Uuid;

impl FileService {
    pub fn write_image(&self, input: &ImageWrite) -> Result<WriteResult> {
        if input.bytes.is_empty()
            || input.bytes.len() > INLINE_IMAGE_WRITE_BYTES
            || !assets::editable_image(&input.mime_type)
        {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Invalid inline image save; limit is 512 KiB.",
            ));
        }
        let begin = BeginWrite {
            path: input.path.clone(),
            size: input.bytes.len() as u64,
            mode: WriteMode::Replace,
            revision: Some(input.revision.clone()),
            content: TransferContent::Image,
            mime_type: Some(input.mime_type.clone()),
        };
        let check = || self.authorized(true);
        let mut stage = StagedWrite::begin(self, &begin, TRANSFER_IMAGE_BYTES, &check)?;
        for (index, chunk) in input.bytes.chunks(CHUNK_BYTES).enumerate() {
            stage.write(
                self,
                &WriteChunk {
                    transfer_id: Uuid::nil(),
                    offset: (index * CHUNK_BYTES) as u64,
                    data: chunk.to_vec(),
                },
                &check,
            )?;
        }
        stage.commit(self, TRANSFER_IMAGE_BYTES, &check)
    }
}
