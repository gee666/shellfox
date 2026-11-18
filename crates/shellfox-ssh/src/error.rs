use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// Fixed messages only. Native errors and parser text can contain credentials.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("Invalid SSH profile or endpoint.")]
    Validation,
    #[error("A connection with this name already exists.")]
    DuplicateName,
    #[error("SSH private storage is unavailable.")]
    Storage,
    #[error("SSH private storage is corrupt; repair it before saving or connecting.")]
    Corrupt,
    #[error("Unsafe SSH storage ownership, permissions, link or file type.")]
    Permissions,
    #[error("Private permissions helper is unavailable or returned invalid output.")]
    PermissionsHelper,
    #[error("SSH storage is busy; retry later.")]
    Busy,
    #[error("SSH input exceeds the supported resource limit.")]
    Limit,
    #[error("Host key changed; verify the replacement out of band before removing trust.")]
    HostKeyChanged,
    #[error("Host key was not explicitly trusted.")]
    HostNotTrusted,
    #[error("Invalid SSH key.")]
    InvalidKey,
    #[error("Encrypted key requires a passphrase.")]
    PassphraseRequired,
    #[error("This private-key format is not yet supported by the Rust adapter.")]
    UnsupportedKeyFormat,
    #[error("SSH process failed or exceeded its deadline/output limit.")]
    Process,
    #[error("SSH connection adapter is not installed; no connection was started.")]
    AdapterUnavailable,
    #[error("SSH proxy or jump host is not supported by this adapter.")]
    UnsupportedProxy,
}

impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Storage
    }
}
