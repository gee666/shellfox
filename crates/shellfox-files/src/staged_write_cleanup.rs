use super::*;
impl Drop for StagedWrite {
    fn drop(&mut self) {
        if let Some(custody) = &self.custody {
            // Only original live user-stage authority can dispose this object. Restart records
            // never reach this path. Unknown/native-rename outcomes preserve instead of retrying.
            let result = (|| {
                let mutation = FileService::mutation_lock()?;
                let disposed = match custody.dispose() {
                    Ok(value) => value,
                    Err(error) => {
                        custody.preserve();
                        if let Some(journal) = &self.journal {
                            if let Ok(mut journal) = journal.lock() {
                                journal.cleanup_error = Some(error);
                            }
                        }
                        false
                    }
                };
                if let Some(journal) = &self.journal {
                    journal
                        .lock()
                        .map_err(|_| crate::journal::invalid())?
                        .finish_stage(
                            &self.target.absolute.with_file_name(&self.name),
                            disposed,
                            &mutation,
                        )?;
                }
                Ok::<_, FileError>(())
            })();
            if let Err(error) = result {
                if let Some(journal) = &self.journal {
                    if let Ok(mut journal) = journal.lock() {
                        journal.cleanup_error = Some(error);
                    }
                }
            }
            return;
        }
        if self.publication_entered {
            return;
        }
        // Existing bounded inline/image compatibility has no restart journal and is not an
        // editor/transfer fallback. It still checks the actual original FD before cleanup.
        if let Ok(current) = content::open_regular(&self.target.parent, &self.name) {
            if content::same_held_file(&current, &self.file).unwrap_or(false) {
                let _ = self.target.parent.remove_file(&self.name);
            }
        }
    }
}
