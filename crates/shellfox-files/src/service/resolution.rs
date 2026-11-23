//! How a request path maps to a directory handle under each kind of root.
use super::*;
use std::path::Component;
#[cfg(test)]
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
            RootResolution::HandleOnly => None,
        }
    }
    pub(crate) fn is_private_scope(&self) -> bool {
        self.root
            .as_ref()
            .is_some_and(|root| matches!(root.resolution, RootResolution::HandleOnly))
    }
    pub(crate) fn require_path_resolution(&self) -> Result<()> {
        if self.is_private_scope() {
            return Err(FileError::new(
                ErrorCode::Unsupported,
                "Paths and working directories are unavailable in a private directory scope.",
            ));
        }
        Ok(())
    }
    pub(crate) fn base(&self, value: &str) -> Result<(Dir, PathBuf, PathBuf)> {
        self.authorized(false)?;
        if let Some(root) = &self.root {
            match &root.resolution {
                RootResolution::HandleOnly => {
                    let relative = native_selector(value)?;
                    // Results name the same relative path back, never a host path.
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
