use crate::{assets, content, ContentKind, ErrorCode, FileError, Result, CHUNK_BYTES};
use cap_std::fs::File;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub(crate) struct Inspected {
    pub size: u64,
    pub revision: String,
    pub kind: ContentKind,
    pub mime_type: String,
    pub snapshot: content::Snapshot,
}

struct Utf8 {
    valid: bool,
    tail: Vec<u8>,
}
impl Utf8 {
    fn push(&mut self, bytes: &[u8]) {
        if !self.valid {
            return;
        }
        if bytes.contains(&0) {
            self.valid = false;
            return;
        }
        let mut combined = std::mem::take(&mut self.tail);
        combined.extend_from_slice(bytes);
        match std::str::from_utf8(&combined) {
            Ok(_) => {}
            Err(error) if error.error_len().is_none() => {
                self.tail
                    .extend_from_slice(&combined[error.valid_up_to()..]);
                if self.tail.len() > 3 {
                    self.valid = false;
                }
            }
            Err(_) => self.valid = false,
        }
    }
}

pub(crate) fn inspect(
    file: &mut File,
    path: &Path,
    limit: u64,
    check: &dyn Fn() -> Result<()>,
) -> Result<Inspected> {
    check()?;
    let snapshot = content::snapshot(file)?;
    let size = snapshot.metadata.len();
    if size > limit {
        return Err(FileError::new(
            ErrorCode::Unsupported,
            "File exceeds the configured transfer limit.",
        ));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut utf8 = Utf8 {
        valid: true,
        tail: Vec::new(),
    };
    let mut buffer = vec![0u8; CHUNK_BYTES];
    let mut offset = 0;
    let mut image = None;
    let mut header = Vec::with_capacity(16);
    while offset < size {
        check()?;
        let wanted = (size - offset).min(CHUNK_BYTES as u64) as usize;
        let count = file.read(&mut buffer[..wanted])?;
        if count == 0 {
            return Err(FileError::conflict());
        }
        let bytes = &buffer[..count];
        if header.len() < 16 {
            header.extend_from_slice(&bytes[..bytes.len().min(16 - header.len())]);
            image = content::image_mime(&header);
        }
        hash.update(bytes);
        utf8.push(bytes);
        offset += count as u64;
    }
    if file.read(&mut buffer[..1])? != 0
        || !content::same_version(&snapshot, &content::snapshot(file)?)
    {
        return Err(FileError::conflict());
    }
    check()?;
    let kind = if image.is_some() {
        ContentKind::Image
    } else if utf8.valid && utf8.tail.is_empty() {
        ContentKind::Text
    } else {
        ContentKind::Binary
    };
    let mime_type = image.unwrap_or_else(|| {
        if kind == ContentKind::Text {
            assets::mime(path)
        } else {
            "application/octet-stream"
        }
    });
    Ok(Inspected {
        size,
        revision: format!("{:x}", hash.finalize()),
        kind,
        mime_type: mime_type.to_owned(),
        snapshot,
    })
}

pub(crate) fn checked_read(
    file: &mut File,
    snapshot: &content::Snapshot,
    offset: u64,
    check: &dyn Fn() -> Result<()>,
) -> Result<Vec<u8>> {
    check()?;
    if !content::same_version(snapshot, &content::snapshot(file)?) {
        return Err(FileError::conflict());
    }
    let size = snapshot.metadata.len();
    if offset > size {
        return Err(FileError::new(
            ErrorCode::Validation,
            "Unexpected download offset.",
        ));
    }
    let mut bytes = vec![0u8; (size - offset).min(CHUNK_BYTES as u64) as usize];
    file.seek(SeekFrom::Start(offset))?;
    let mut read = 0;
    while read < bytes.len() {
        check()?;
        let count = file.read(&mut bytes[read..])?;
        if count == 0 {
            return Err(FileError::conflict());
        }
        read += count;
    }
    if !content::same_version(snapshot, &content::snapshot(file)?) {
        return Err(FileError::conflict());
    }
    check()?;
    Ok(bytes)
}
