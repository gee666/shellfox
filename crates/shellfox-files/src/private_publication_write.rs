use super::*;
use std::io::Write;
impl OwnedPrivatePublication {
    #[cfg(test)]
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        self.write_checked(bytes, || Ok(()))
    }
    pub(crate) fn write_checked(
        &mut self,
        bytes: &[u8],
        check: impl Fn() -> Result<()>,
    ) -> Result<()> {
        self.write_with(bytes, |file, bytes| {
            check()?;
            let mut file = file;
            file.write_all(bytes)?;
            Ok(())
        })
    }
    pub(super) fn write_with(
        &mut self,
        bytes: &[u8],
        write: impl FnOnce(&File, &[u8]) -> Result<()>,
    ) -> Result<()> {
        if self.phase != PrivatePublicationPhase::Preparing {
            return Err(state_error());
        }
        self.phase = PrivatePublicationPhase::WriteAttempted; // one-shot, BEFORE any I/O
        if bytes.len() > PRIVATE_METADATA_MAX_BYTES {
            self.phase = PrivatePublicationPhase::NotPublished;
            return Err(FileError::new(
                ErrorCode::Validation,
                "Private metadata exceeds its byte limit.",
            ));
        }
        let Some(stage) = &self.stage else {
            self.phase = PrivatePublicationPhase::WriteFailed;
            return Err(state_error());
        };
        self.owned_version = None; // a partial error is not the old empty-file version
        let outcome = (|| {
            stage.verify_binding()?;
            write(stage.as_file(), bytes)?;
            let read = read_file(stage.as_file(), PRIVATE_METADATA_MAX_BYTES)?;
            if read.bytes != bytes || read.version.identity() != stage.identity() {
                return Err(FileError::conflict());
            }
            Ok(read.version)
        })();
        match outcome {
            Ok(version) => {
                self.owned_version = Some(version);
                self.phase = PrivatePublicationPhase::Ready;
                Ok(())
            }
            Err(error) => {
                self.phase = PrivatePublicationPhase::WriteFailed;
                Err(error)
            }
        }
    }
}
