//! Native labels never become path authority; normal path-root behavior is separate.
use super::*;
use std::path::Component;
#[cfg(test)]
#[path = "service_resolution_tests.rs"]
mod tests;
fn native_selector(value: &str) -> Result<PathBuf> {
    paths::validate(value)?;
    // Portable denial, not host-dependent Prefix parsing (C: is relative on Unix).
    if value.contains(['\\', ':']) || value.starts_with('/') {
        return Err(FileError::denied());
    }
    let mut relative = PathBuf::new();
    for component in Path::new(value).components() {
        match component {
            Component::Normal(name) => relative.push(name),
            Component::CurDir => {}
            _ => return Err(FileError::denied()),
        }
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Ok(relative)
}
impl FileService {
    pub fn root_path(&self) -> Option<&Path> {
        match &self.root.as_ref()?.resolution {
            RootResolution::PathBound { path } => Some(path),
            RootResolution::AnchoredRelative { .. } => None,
        }
    }
    pub(crate) fn diagnostic_label(&self) -> Option<&str> {
        match &self.root.as_ref()?.resolution {
            RootResolution::AnchoredRelative { diagnostic_label } => Some(diagnostic_label),
            RootResolution::PathBound { .. } => None,
        }
    }
    pub(crate) fn is_anchored_relative(&self) -> bool {
        self.diagnostic_label().is_some()
    }
    pub(crate) fn require_path_resolution(&self) -> Result<()> {
        if self.is_anchored_relative() {
            return Err(FileError::new(
                ErrorCode::Unsupported,
                "Path/cwd planning is unavailable for an anchored-relative native scope.",
            ));
        }
        Ok(())
    }
    pub(crate) fn retained_anchored_directory(&self) -> Result<Dir> {
        self.authorized(true)?;
        let root = self.root.as_ref().ok_or_else(FileError::denied)?;
        if !self.is_anchored_relative() {
            return Err(FileError::new(
                ErrorCode::Unsupported,
                "A retained anchored-relative native directory is required.",
            ));
        }
        Ok(root.dir.try_clone()?)
    }
    pub(crate) fn derive_anchored_relative(
        &self,
        dir: Dir,
        diagnostic_label: String,
        fence: Arc<dyn NativeCommitFence>,
    ) -> Result<FileService> {
        self.can_derive_root()?;
        if diagnostic_label.is_empty()
            || diagnostic_label.len() > 1024
            || diagnostic_label.chars().any(char::is_control)
        {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Invalid native diagnostic label.",
            ));
        }
        let metadata = dir.dir_metadata()?;
        paths::reject_link(&metadata)?;
        if !metadata.is_dir() {
            return Err(FileError::denied());
        }
        HandleIdentity::from_directory(&dir)?;
        self.derive_bound_root(
            Root {
                dir,
                resolution: RootResolution::AnchoredRelative { diagnostic_label },
            },
            Some(fence),
        )
    }
    pub(crate) fn base(&self, value: &str) -> Result<(Dir, PathBuf, PathBuf)> {
        self.authorized(false)?;
        if let Some(root) = &self.root {
            match &root.resolution {
                RootResolution::AnchoredRelative { .. } => {
                    let relative = native_selector(value)?;
                    // Output is a relative selector too, NEVER the diagnostic label.
                    return Ok((root.dir.try_clone()?, relative.clone(), relative));
                }
                RootResolution::PathBound { path } => {
                    let requested = Path::new(value);
                    let relative = if requested.is_absolute() {
                        requested
                            .strip_prefix(path)
                            .map_err(|_| FileError::denied())?
                    } else {
                        requested
                    };
                    paths::validate(
                        relative
                            .to_str()
                            .filter(|part| !part.is_empty())
                            .unwrap_or("."),
                    )?;
                    paths::validate(value)?;
                    let now = paths::open_absolute(path)?;
                    if HandleIdentity::from_directory(&root.dir)?
                        != HandleIdentity::from_directory(&now)?
                    {
                        return Err(FileError::conflict());
                    }
                    return Ok((
                        root.dir.try_clone()?,
                        relative.to_path_buf(),
                        path.join(relative),
                    ));
                }
            }
        }
        paths::validate(value)?;
        let requested = Path::new(value);
        if !requested.is_absolute() {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Host account paths must be absolute.",
            ));
        }
        let path = requested.ancestors().last().ok_or_else(FileError::denied)?;
        let dir = paths::open_absolute(path)?;
        let relative = requested
            .strip_prefix(path)
            .map_err(|_| FileError::denied())?;
        Ok((dir, relative.to_path_buf(), requested.to_path_buf()))
    }
}
