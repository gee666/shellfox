use crate::{assets, content, paths, FileService, Location, Result, PREVIEW_BYTES};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReadResult {
    Text {
        path: String,
        mime_type: String,
        size: u64,
        revision: String,
        text: String,
    },
    Image {
        path: String,
        mime_type: String,
        size: u64,
        revision: String,
        bytes: Vec<u8>,
    },
    Binary {
        path: String,
        mime_type: String,
        size: u64,
        revision: String,
    },
}
impl FileService {
    /// Bounded inline read with the detected MIME type; binary files return metadata only.
    pub fn read_file(&self, input: &Location) -> Result<ReadResult> {
        let target = self.target(&input.path)?;
        let (bytes, _) = content::read(
            &mut content::open_regular(&target.parent, &target.name)?,
            PREVIEW_BYTES,
        )?;
        self.authorized(false)?;
        let path = paths::display(&target.absolute)?;
        let revision = content::revision(&bytes);
        let size = bytes.len() as u64;
        if let Some(mime_type) = content::image_mime(&bytes) {
            return Ok(ReadResult::Image {
                path,
                mime_type: mime_type.to_owned(),
                size,
                revision,
                bytes,
            });
        }
        if let Ok(text) = content::text(&bytes) {
            return Ok(ReadResult::Text {
                path,
                mime_type: assets::mime(&target.absolute).to_owned(),
                size,
                revision,
                text,
            });
        }
        Ok(ReadResult::Binary {
            path,
            mime_type: "application/octet-stream".to_owned(),
            size,
            revision,
        })
    }
}
