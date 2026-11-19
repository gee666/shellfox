//! Same-account SSH profile reads. Neither request is in the remote protocol.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use shellfox_protocol::{validation::ensure, LocalRequest, Validate, ValidationError};
use shellfox_ssh::{
    PrivateDirectory, ProfileDto, ProfileInput, ProfileStore, StoragePolicy, StoredProfile,
};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum SshRead {
    List {},
    Connection { id: Uuid },
}
impl Validate for SshRead {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(!matches!(self, Self::Connection { id } if id.is_nil()))
    }
}
impl LocalRequest for SshRead {
    type Response = SshReadReply;
    const METHOD: &str = shellfox_protocol::SSH_READ_NAMESPACE;
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum SshReadReply {
    Profiles(Vec<ProfileDto>),
    Connection(Option<Box<StoredProfile>>),
}
impl Validate for SshReadReply {
    fn validate(&self) -> Result<(), ValidationError> {
        fn profile(p: &ProfileDto) -> Result<(), ValidationError> {
            ensure(!p.id.is_nil())?;
            ProfileInput {
                id: p.id,
                name: p.name.clone(),
                host: p.host.clone(),
                port: p.port,
                user: p.user.clone(),
                password: Default::default(),
                key_file: p.key_file.clone(),
                remote_cwd: p.remote_cwd.clone(),
            }
            .validate()
            .map_err(|_| ValidationError)?;
            Ok(())
        }
        match self {
            Self::Profiles(profiles) => {
                ensure(profiles.len() <= 10_000)?;
                for p in profiles {
                    profile(p)?;
                }
            }
            Self::Connection(Some(p)) => {
                profile(&p.dto())?;
                p.validate_connection_options()
                    .map_err(|_| ValidationError)?;
            }
            Self::Connection(None) => {}
        }
        Ok(())
    }
}

/// An absent endpoint record is the only reason to select the offline path.
/// A stale, unreadable, malformed or unauthenticated endpoint is not absence.
pub fn ssh_endpoint_exists(data: &Path) -> Result<bool> {
    use crate::security::{reject_links, HostRoot, ENDPOINT_DIRECTORY};
    let root = HostRoot::inspect(data)?;
    root.revalidate()?;
    let endpoint_dir = root.path().join(ENDPOINT_DIRECTORY);
    match reject_links(&endpoint_dir, true) {
        Err(error) if missing(&error) => return Ok(false),
        result => result?,
    }
    match std::fs::symlink_metadata(endpoint_dir.join("endpoint.json")) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
fn missing(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|e| e.kind() == ErrorKind::NotFound)
}

#[derive(Clone)]
pub(crate) struct SshStoreLocation {
    pub(crate) endpoint_directory: PathBuf,
    pub(crate) helper: (PathBuf, [u8; 32]),
}
impl SshStoreLocation {
    pub(crate) fn install(self) -> Result<crate::ssh::Store> {
        let policy = storage_policy(Some(&self.helper))?;
        let directory = PrivateDirectory::open(self.endpoint_directory.join("ssh"), policy)?;
        Ok(crate::ssh::Store::new(directory))
    }
}
pub fn open_ssh_directory(
    data: &Path,
    helper: Option<&(PathBuf, [u8; 32])>,
) -> Result<Option<Arc<PrivateDirectory>>> {
    let root = crate::security::HostRoot::inspect(data)?;
    root.revalidate()?;
    open_ssh_directory_at(
        &root.path().join(crate::security::ENDPOINT_DIRECTORY),
        helper,
    )
}
fn open_ssh_directory_at(
    endpoint_directory: &Path,
    helper: Option<&(PathBuf, [u8; 32])>,
) -> Result<Option<Arc<PrivateDirectory>>> {
    let path = endpoint_directory.join("ssh");
    for dir in [endpoint_directory, path.as_path()] {
        match crate::security::reject_links(dir, true) {
            Err(error) if missing(&error) => return Ok(None),
            result => result?,
        }
    }
    Ok(Some(Arc::new(PrivateDirectory::open_existing(
        path,
        storage_policy(helper)?,
    )?)))
}
fn storage_policy(helper: Option<&(PathBuf, [u8; 32])>) -> Result<StoragePolicy> {
    if cfg!(windows) {
        let (path, digest) =
            helper.context("A trusted helper is required to read the private SSH directory.")?;
        crate::native_providers::ssh_storage_policy(path, *digest)
    } else {
        Ok(StoragePolicy::default())
    }
}
pub fn read_ssh_profiles(
    directory: Option<&Arc<PrivateDirectory>>,
    request: SshRead,
) -> Result<SshReadReply> {
    let store = directory.map(|directory| ProfileStore::new(directory.clone()));
    match request {
        SshRead::List {} => Ok(SshReadReply::Profiles(
            store
                .map(|s| s.list_for_cli())
                .transpose()?
                .unwrap_or_default(),
        )),
        SshRead::Connection { id } => Ok(SshReadReply::Connection(
            store
                .map(|s| s.get_for_connection(id))
                .transpose()?
                .flatten()
                .map(Box::new),
        )),
    }
}
