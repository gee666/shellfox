use crate::{HelperError, Secret};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    net::IpAddr,
    path::{Path, PathBuf},
};

/// An explicit immutable helper artifact, checked again before every spawn.
#[derive(Clone)]
pub struct VerifiedHelper {
    path: PathBuf,
    sha256: [u8; 32],
    _resource_guard: Option<std::sync::Arc<dyn Send + Sync>>,
}
impl std::fmt::Debug for VerifiedHelper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedHelper")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}
impl VerifiedHelper {
    pub fn new(path: PathBuf, sha256: [u8; 32]) -> Result<Self, HelperError> {
        let helper = Self {
            path,
            sha256,
            _resource_guard: None,
        };
        helper.verify()?;
        Ok(helper)
    }
    /// Keep a caller's validated artifact bundle/cache lease alive with every
    /// original child, including rollback and detached confirmed cleanup.
    pub fn with_resource_guard(mut self, guard: std::sync::Arc<dyn Send + Sync>) -> Self {
        self._resource_guard = Some(guard);
        self
    }
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn verify(&self) -> Result<(), HelperError> {
        self.read_checked(256 * 1024 * 1024, false).map(|_| ())
    }
    /// Return only the bounded bytes hashed against the external expected digest
    /// from this SAME opened descriptor. The resource guard stays retained for
    /// the operation. Errors, overflow or observed changes return no partial data.
    pub fn read_verified(&self, maximum: usize) -> Result<Vec<u8>, HelperError> {
        self.read_checked(maximum, true)
    }
    /// Validate the actual held file before it becomes a caller's resource lease.
    /// This returns that same owned descriptor, never a fabricated path proof.
    pub fn verify_held_file(&self, mut file: fs::File) -> Result<fs::File, HelperError> {
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0))
            .map_err(|_| HelperError::Executable)?;
        self.read_descriptor(&mut file, 256 * 1024 * 1024, false)?;
        Ok(file)
    }
    fn read_checked(&self, maximum: usize, collect: bool) -> Result<Vec<u8>, HelperError> {
        let info = fs::symlink_metadata(&self.path).map_err(|_| HelperError::Executable)?;
        if maximum == 0
            || maximum > 256 * 1024 * 1024
            || !self.path.is_absolute()
            || !info.is_file()
            || info.file_type().is_symlink()
            || info.len() > maximum as u64
        {
            return Err(HelperError::Executable);
        }
        let mut file = fs::File::open(&self.path).map_err(|_| HelperError::Executable)?;
        let before = file.metadata().map_err(|_| HelperError::Executable)?;
        let modified = before.modified().map_err(|_| HelperError::Executable)?;
        if !before.is_file()
            || before.len() != info.len()
            || info.modified().map_err(|_| HelperError::Executable)? != modified
        {
            return Err(HelperError::Executable);
        }
        self.read_descriptor(&mut file, maximum, collect)
    }
    fn read_descriptor(
        &self,
        file: &mut fs::File,
        maximum: usize,
        collect: bool,
    ) -> Result<Vec<u8>, HelperError> {
        let before = file.metadata().map_err(|_| HelperError::Executable)?;
        if maximum == 0
            || maximum > 256 * 1024 * 1024
            || !before.is_file()
            || before.len() > maximum as u64
        {
            return Err(HelperError::Executable);
        }
        let modified = before.modified().map_err(|_| HelperError::Executable)?;
        let mut hash = Sha256::new();
        let mut bytes = Vec::new();
        let mut buf = [0; 16 * 1024];
        let mut total = 0usize;
        loop {
            let n = file.read(&mut buf).map_err(|_| HelperError::Executable)?;
            if n == 0 {
                break;
            }
            total = total.checked_add(n).ok_or(HelperError::Executable)?;
            if total > maximum {
                return Err(HelperError::Executable);
            }
            hash.update(&buf[..n]);
            if collect {
                bytes.extend_from_slice(&buf[..n]);
            }
        }
        let after = file.metadata().map_err(|_| HelperError::Executable)?;
        if after.len() != before.len()
            || total as u64 != before.len()
            || after.modified().map_err(|_| HelperError::Executable)? != modified
            || hash.finalize().as_slice() != self.sha256
        {
            return Err(HelperError::Executable);
        }
        Ok(bytes)
    }
}

mod state_directory;
pub use state_directory::IsolatedStateDirectory;

#[derive(Clone, Debug)]
pub struct LoopbackOrigin {
    pub(crate) address: std::net::SocketAddr,
}
impl LoopbackOrigin {
    pub fn new(address: std::net::SocketAddr) -> Result<Self, HelperError> {
        let mapped = matches!(address.ip(), IpAddr::V6(ip) if ip.to_ipv4_mapped().is_some());
        if !address.ip().is_loopback() || address.port() == 0 || mapped {
            return Err(HelperError::Configuration);
        }
        Ok(Self { address })
    }
    pub(crate) fn url(&self) -> String {
        format!("http://{}", self.address)
    }
    /// Trusted operational override for an isolated local control server only.
    /// No DNS, userinfo, query, fragment, encoded/scoped address, or remote URL.
    pub fn parse_control_url(value: &str) -> Result<Self, HelperError> {
        if value.len() > 128 || value.contains('%') {
            return Err(HelperError::Configuration);
        }
        let origin = value
            .strip_prefix("http://")
            .ok_or(HelperError::Configuration)?;
        let address = origin
            .strip_suffix('/')
            .unwrap_or(origin)
            .parse()
            .map_err(|_| HelperError::Configuration)?;
        Self::new(address)
    }
    pub(crate) fn parse_proxy(value: &str) -> Result<Self, HelperError> {
        let result = Self::parse_control_url(value).map_err(|_| HelperError::Protocol)?;
        if !matches!(result.address.ip(), IpAddr::V4(ip) if ip.octets() == [127,0,0,1]) {
            return Err(HelperError::Protocol);
        }
        Ok(result)
    }
}

#[derive(Debug)]
pub struct HelperConfig {
    pub executable: VerifiedHelper,
    pub state_directory: IsolatedStateDirectory,
    pub hostname: String,
    pub backend: LoopbackOrigin,
    pub backend_secret: Secret,
    pub proxy_secret: Secret,
}
impl HelperConfig {
    pub(crate) fn validate(&self) -> Result<(), HelperError> {
        if self.hostname.is_empty()
            || self.hostname.len() > 63
            || !self.hostname.as_bytes()[0].is_ascii_alphanumeric()
            || !self
                .hostname
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || self.backend_secret.expose() == self.proxy_secret.expose()
        {
            return Err(HelperError::Configuration);
        }
        Ok(())
    }
}
