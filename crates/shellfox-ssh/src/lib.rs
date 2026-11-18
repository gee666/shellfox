//! Host-only SSH profiles, private persistence and explicit host-key trust.
//! Do not link this crate into remote-only mobile clients. Export ProfileDto only.
mod endpoint;
mod error;
pub mod key_metadata;
mod key_pem;
mod key_ppk;
mod key_validation;
pub mod keys;
mod known_hosts;
mod model;
mod private;
mod private_platform;
mod process;
mod profiles;
pub mod pty;
pub mod system;
mod system_proof;
pub mod transport;

pub use endpoint::normalize_endpoint;
pub use error::{Error, Result};
pub use known_hosts::{host_key_info, HostKeyInfo, KnownHosts, TrustDecision, TrustStatus};
pub use model::{
    ConnectionOptions, ImportResult, ImportSession, PasswordUpdate, ProfileDto, ProfileInput,
    ProfileSource, RemoteProfileDto, Secret, StoredProfile, UnsupportedProxy,
};
pub use private::{PrivateDirectory, StoragePolicy};
pub use process::{ProcessOutput, ProcessRequest, ProcessRunner, SystemProcessRunner};
pub use profiles::ProfileStore;
