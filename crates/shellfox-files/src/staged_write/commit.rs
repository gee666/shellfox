use super::*;
impl StagedWrite {
    pub fn commit(
        &mut self,
        service: &FileService,
        max: u64,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<WriteResult> {
        self.commit_with_hook(service, max, check, || {})
    }
    pub(super) fn commit_with_hook(
        &mut self,
        service: &FileService,
        max: u64,
        check: &dyn Fn() -> Result<()>,
        before_publish: impl FnOnce(),
    ) -> Result<WriteResult> {
        let _guard = FileService::mutation_lock()?;
        check()?;
        service.authorized(true)?;
        if self.offset != self.input.size {
            return Err(invalid("Upload is incomplete."));
        }
        self.verify(service)?;
        let inspected = stream_io::inspect(&mut self.file, &self.target.absolute, max, check)?;
        if inspected.revision != format!("{:x}", self.hash.clone().finalize()) {
            return Err(FileError::conflict());
        }
        if self.input.content == TransferContent::Text && inspected.kind != ContentKind::Text {
            return Err(FileError::new(
                ErrorCode::Unsupported,
                "Only UTF-8 text without NUL can be saved by the editor.",
            ));
        }
        if self.input.content == TransferContent::Image
            && (inspected.kind != ContentKind::Image
                || self.input.mime_type.as_deref() != Some(&inspected.mime_type)
                || assets::mime(&self.target.absolute) != inspected.mime_type)
        {
            return Err(invalid(
                "Image MIME, extension and magic header must match.",
            ));
        }
        if let Some((previous, info)) = &self.previous {
            let mut current = content::open_regular(&self.target.parent, &self.target.name)?;
            let current = stream_io::inspect(&mut current, &self.target.absolute, max, check)?;
            if !content::same_version(info, &current.snapshot)
                || self.input.revision.as_deref() != Some(&current.revision)
            {
                return Err(FileError::conflict());
            }
            platform::preserve_permissions(previous, &self.file)?;
        } else {
            absent(&self.target)?;
        }
        self.file.sync_all()?;
        self.verify(service)?;
        let ready = content::snapshot(&self.file)?;
        let written = WriteResult {
            path: paths::display(&self.target.absolute)?,
            size: inspected.size,
            revision: inspected.revision,
        };
        before_publish();
        if let Some((_, info)) = &self.previous {
            let mut current = content::open_regular(&self.target.parent, &self.target.name)?;
            let current = stream_io::inspect(&mut current, &self.target.absolute, max, check)?;
            if !content::same_version(info, &current.snapshot)
                || self.input.revision.as_deref() != Some(&current.revision)
            {
                return Err(FileError::conflict());
            }
        } else {
            absent(&self.target)?;
        }
        self.verify(service)?;
        if !content::same_version(&ready, &content::snapshot(&self.file)?) {
            return Err(FileError::conflict());
        }
        // Last authorization check, after inspection, permission copy and fsync. The caller
        // holds its approval guard through the rename.
        check()?;
        service.authorized(true)?;
        self.publication_entered = true;
        self.publish_stage(service, check)?;
        crate::private::sync_dir(&self.target.parent);
        Ok(written)
    }
    fn publish_stage(&self, service: &FileService, check: &dyn Fn() -> Result<()>) -> Result<()> {
        let source = Target {
            parent: self.target.parent.try_clone()?,
            name: self.name.clone(),
            absolute: self.target.absolute.with_file_name(&self.name),
        };
        let final_check = || {
            check()?;
            service.recheck_parent(&self.input.path, &self.target)?;
            service.authorized(true)
        };
        if self.previous.is_some() {
            platform::rename_user_replace_checked(&source, &self.target, &self.file, &final_check)
        } else {
            platform::rename_user_new_checked(&source, &self.target, &self.file, &final_check)
        }
    }
}
