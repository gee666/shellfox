use super::*;
pub(super) struct ExistingLocalDirectory;
impl LaunchDirectoryPolicy for ExistingLocalDirectory {
    fn validate_directory(&self, cwd: &str) -> std::result::Result<String, LaunchError> {
        shellfox_core::validation::cwd(cwd).map_err(|_| LaunchError::Invalid)?;
        let path = std::fs::canonicalize(cwd).map_err(|_| LaunchError::NotFound)?;
        #[cfg(windows)]
        let path =
            shellfox_transport::normalize_private_path(&path).map_err(|_| LaunchError::Invalid)?;
        if !path.is_dir() {
            return Err(LaunchError::NotFound);
        }
        let cwd = path.to_str().ok_or(LaunchError::Invalid)?.to_owned();
        // Revalidate the resolved target: directory aliases must not turn a
        // permitted local-drive/WSL spelling into an arbitrary network share.
        shellfox_core::validation::cwd(&cwd).map_err(|_| LaunchError::Invalid)?;
        Ok(cwd)
    }
}
