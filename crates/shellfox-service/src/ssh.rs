//! Local SSH profiles and key validation. Connection state belongs to the CLI.
#[path = "ssh_api.rs"]
mod api;
#[path = "native_provider_putty.rs"]
mod putty;
use anyhow::{Context, Result};
pub use api::{Input, Password, Request, Response, SelectedKey, SelectedKeyInfo};
pub use shellfox_ssh::key_metadata::{KeyFormat, KeyMetadata};
pub use shellfox_ssh::Secret;
use shellfox_ssh::{Error, PrivateDirectory, ProfileStore};
use std::sync::Arc;

pub(crate) struct Store {
    directory: Arc<PrivateDirectory>,
    profiles: ProfileStore,
}
impl Store {
    pub(crate) fn new(directory: PrivateDirectory) -> Self {
        let directory = Arc::new(directory);
        Self {
            profiles: ProfileStore::new(directory.clone()),
            directory,
        }
    }
    pub(crate) fn read(
        &self,
        request: crate::local::SshRead,
    ) -> Result<crate::local::SshReadReply> {
        crate::local::read_ssh_profiles(Some(&self.directory), request)
    }
    fn request(
        &self,
        request: Request,
        check: impl Fn() -> shellfox_ssh::Result<()>,
    ) -> Result<Response> {
        request.validate()?;
        check()?;
        match request {
            Request::List => Ok(Response::Profiles(self.profiles.list()?)),
            Request::Save(input) => Ok(Response::Profiles(
                self.profiles.save_guarded(input.into_profile(), check)?,
            )),
            Request::Delete { id } => {
                Ok(Response::Profiles(self.profiles.delete_guarded(id, check)?))
            }
            Request::Import => {
                let sessions = putty::import_sessions().context("Native SSH import failed.")?;
                Ok(Response::Imported(
                    self.profiles.import_guarded(sessions, check)?,
                ))
            }
            Request::ValidateKey { selected } => {
                let bytes = self.directory.read_key_file(selected.path())?;
                let metadata = shellfox_ssh::key_metadata::inspect(&bytes)?;
                let cryptographically_validated =
                    match shellfox_ssh::keys::parse_private_key(&bytes, None) {
                        Ok(_) => true,
                        Err(Error::PassphraseRequired) => false,
                        Err(Error::UnsupportedKeyFormat) if metadata.limitation.is_some() => false,
                        Err(Error::InvalidKey)
                            if metadata.encrypted && metadata.limitation.is_some() =>
                        {
                            false
                        }
                        Err(error) => return Err(error.into()),
                    };
                check()?;
                let path = selected
                    .path()
                    .to_str()
                    .context("Unsupported SSH key path encoding.")?
                    .to_owned();
                Ok(Response::SelectedKey(SelectedKeyInfo {
                    path,
                    metadata,
                    cryptographically_validated,
                }))
            }
        }
    }
}
impl crate::Service {
    pub(crate) fn execute_ssh_settings(&self, request: Request) -> Result<Response> {
        self.assert_accepting()?;
        let store = self
            .ssh_profiles
            .as_ref()
            .context("SSH profile storage is unavailable.")?;
        store.request(request, || {
            if self.grant_recipient.is_cancelled() {
                return Err(Error::Permissions);
            }
            self.assert_accepting().map_err(|_| Error::Permissions)
        })
    }
    pub(crate) fn read_ssh_settings(
        &self,
        request: crate::local::SshRead,
    ) -> Result<crate::local::SshReadReply> {
        self.assert_accepting()?;
        self.ssh_profiles
            .as_ref()
            .context("SSH profile storage is unavailable.")?
            .read(request)
    }
}
