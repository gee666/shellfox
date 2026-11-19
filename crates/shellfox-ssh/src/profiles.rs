use crate::{
    Error, ImportResult, ImportSession, PasswordUpdate, PrivateDirectory, ProfileDto, ProfileInput,
    ProfileSource, Result, StoredProfile,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;
use zeroize::Zeroizing;

const FILE: &str = "ssh-profiles.json";
const LOCK: &str = "ssh-profiles.lock";
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_PROFILES: usize = 10000;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    profiles: Vec<StoredProfile>,
}
struct Loaded {
    profiles: Vec<StoredProfile>,
    normalized: HashSet<Uuid>,
}

#[derive(Clone)]
pub struct ProfileStore {
    directory: Arc<PrivateDirectory>,
}
impl ProfileStore {
    pub fn new(directory: Arc<PrivateDirectory>) -> Self {
        Self { directory }
    }
    fn load(&self) -> Result<Loaded> {
        let Some(bytes) = self.directory.read(FILE, MAX_BYTES)? else {
            return Ok(Loaded {
                profiles: vec![],
                normalized: HashSet::new(),
            });
        };
        let document: Document = serde_json::from_slice(&bytes).map_err(|_| Error::Corrupt)?;
        if document.version != 1 || document.profiles.len() > MAX_PROFILES {
            return Err(Error::Corrupt);
        }
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        let mut normalized = HashSet::new();
        let mut profiles = Vec::with_capacity(document.profiles.len());
        for raw in document.profiles {
            let before = (raw.name.clone(), raw.host.clone(), raw.user.clone());
            let p = raw.normalize().map_err(|_| Error::Corrupt)?;
            if !ids.insert(p.id) || !names.insert(p.name.to_lowercase()) {
                return Err(Error::Corrupt);
            }
            if before != (p.name.clone(), p.host.clone(), p.user.clone()) {
                normalized.insert(p.id);
            }
            profiles.push(p);
        }
        Ok(Loaded {
            profiles,
            normalized,
        })
    }
    fn write(&self, profiles: Vec<StoredProfile>) -> Result<Vec<ProfileDto>> {
        if profiles.len() > MAX_PROFILES {
            return Err(Error::Limit);
        }
        let dtos = public_profiles(&profiles);
        let bytes = Zeroizing::new(
            serde_json::to_vec_pretty(&Document {
                version: 1,
                profiles,
            })
            .map_err(|_| Error::Storage)?,
        );
        self.directory.atomic_write(FILE, &bytes, MAX_BYTES)?;
        Ok(dtos)
    }
    pub fn list_remote(&self) -> Result<Vec<crate::RemoteProfileDto>> {
        let _lock = self.directory.lock(LOCK)?;
        let mut profiles = self.load()?.profiles;
        profiles.sort_by_key(|p| p.name.to_lowercase());
        Ok(profiles.iter().map(StoredProfile::remote_dto).collect())
    }
    pub fn list(&self) -> Result<Vec<ProfileDto>> {
        let _lock = self.directory.lock(LOCK)?;
        Ok(public_profiles(&self.load()?.profiles))
    }
    /// The writer replaces the entire document atomically. CLI readers do not
    /// take a write lock, so offline profile lookup never modifies the store.
    pub fn list_for_cli(&self) -> Result<Vec<ProfileDto>> {
        Ok(public_profiles(&self.load()?.profiles))
    }
    /// Credential lookup only. Never expose this to a remote protocol.
    pub fn get_for_connection(&self, id: Uuid) -> Result<Option<StoredProfile>> {
        Ok(self.load()?.profiles.into_iter().find(|p| p.id == id))
    }
    pub fn save(&self, input: ProfileInput) -> Result<Vec<ProfileDto>> {
        self.save_guarded(input, || Ok(()))
    }
    pub fn save_guarded(
        &self,
        input: ProfileInput,
        check: impl FnOnce() -> Result<()>,
    ) -> Result<Vec<ProfileDto>> {
        let input = input.validate()?;
        let _lock = self.directory.lock(LOCK)?;
        let mut profiles = self.load()?.profiles;
        if profiles
            .iter()
            .any(|p| p.id != input.id && p.name.to_lowercase() == input.name.to_lowercase())
        {
            return Err(Error::DuplicateName);
        }
        let previous = profiles
            .iter()
            .position(|p| p.id == input.id)
            .map(|i| profiles.remove(i));
        let password = match input.password {
            PasswordUpdate::Keep => previous.as_ref().and_then(|p| p.password.clone()),
            PasswordUpdate::Clear => None,
            PasswordUpdate::Set(value) => Some(value),
        };
        profiles.push(StoredProfile {
            id: input.id,
            name: input.name,
            host: input.host,
            port: input.port,
            user: input.user,
            password,
            key_file: input.key_file,
            remote_cwd: input.remote_cwd,
            source: previous
                .as_ref()
                .map_or(ProfileSource::Manual, |p| p.source),
            connection_options: previous.and_then(|p| p.connection_options),
        });
        check()?;
        self.write(profiles)
    }
    pub fn delete(&self, id: Uuid) -> Result<Vec<ProfileDto>> {
        self.delete_guarded(id, || Ok(()))
    }
    pub fn delete_guarded(
        &self,
        id: Uuid,
        check: impl FnOnce() -> Result<()>,
    ) -> Result<Vec<ProfileDto>> {
        let _lock = self.directory.lock(LOCK)?;
        let mut profiles = self.load()?.profiles;
        profiles.retain(|p| p.id != id);
        check()?;
        self.write(profiles)
    }
    /// Import the Electron v1 document without replacing independently saved profiles.
    /// This is a host-only API; the input includes credentials.
    pub fn import_electron(&self, bytes: &[u8]) -> Result<usize> {
        if bytes.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        let incoming: Document = serde_json::from_slice(bytes).map_err(|_| Error::Corrupt)?;
        if incoming.version != 1 || incoming.profiles.len() > MAX_PROFILES {
            return Err(Error::Corrupt);
        }
        let _lock = self.directory.lock(LOCK)?;
        let mut profiles = self.load()?.profiles;
        let mut ids: HashSet<_> = profiles.iter().map(|p| p.id).collect();
        let mut names: HashSet<_> = profiles.iter().map(|p| p.name.to_lowercase()).collect();
        let old_len = profiles.len();
        for profile in incoming.profiles {
            let profile = profile.normalize()?;
            let name = profile.name.to_lowercase();
            if !ids.contains(&profile.id) && !names.contains(&name) {
                ids.insert(profile.id);
                names.insert(name);
                profiles.push(profile);
            }
        }
        let added = profiles.len() - old_len;
        if added != 0 {
            self.write(profiles)?;
        }
        Ok(added)
    }
    pub fn import_json(&self, bytes: &[u8]) -> Result<ImportResult> {
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(Error::Limit);
        }
        let sessions =
            serde_json::from_slice::<Vec<ImportSession>>(bytes).map_err(|_| Error::Validation)?;
        self.import_sessions(sessions)
    }
    /// Explicitly supplied PuTTY sessions only. No filesystem/registry discovery.
    pub fn import_sessions(&self, sessions: Vec<ImportSession>) -> Result<ImportResult> {
        self.import_guarded(sessions, || Ok(()))
    }
    pub fn import_guarded(
        &self,
        sessions: Vec<ImportSession>,
        check: impl FnOnce() -> Result<()>,
    ) -> Result<ImportResult> {
        if sessions.len() > MAX_PROFILES {
            return Err(Error::Limit);
        }
        let _lock = self.directory.lock(LOCK)?;
        let loaded = self.load()?;
        let mut profiles = loaded.profiles;
        let (mut added, mut updated, mut found) = (0, 0, 0);
        for session in sessions {
            let Ok(input) = (ProfileInput {
                id: Uuid::new_v4(),
                name: session.name,
                host: session.host,
                port: session.port,
                user: session.user,
                password: PasswordUpdate::Keep,
                key_file: session.key_file,
                remote_cwd: None,
            })
            .validate() else {
                continue;
            };
            if session
                .connection_options
                .as_ref()
                .is_some_and(|o| o.validate().is_err())
            {
                continue;
            }
            found += 1;
            if let Some(old) = profiles
                .iter_mut()
                .find(|p| p.name.to_lowercase() == input.name.to_lowercase())
            {
                if loaded.normalized.contains(&old.id)
                    || old.host != input.host
                    || old.port != input.port
                    || old.user != input.user
                    || old.key_file != input.key_file
                    || old.connection_options.as_ref().cloned().unwrap_or_default()
                        != session
                            .connection_options
                            .as_ref()
                            .cloned()
                            .unwrap_or_default()
                {
                    old.host = input.host;
                    old.port = input.port;
                    old.user = input.user;
                    old.key_file = input.key_file;
                    old.connection_options = session.connection_options;
                    updated += 1;
                }
            } else {
                if profiles.len() >= MAX_PROFILES {
                    return Err(Error::Limit);
                }
                profiles.push(StoredProfile {
                    id: input.id,
                    name: input.name,
                    host: input.host,
                    port: input.port,
                    user: input.user,
                    password: None,
                    key_file: input.key_file,
                    remote_cwd: None,
                    source: ProfileSource::Putty,
                    connection_options: session.connection_options,
                });
                added += 1;
            }
        }
        check()?;
        let profiles = if added + updated > 0 {
            self.write(profiles)?
        } else {
            public_profiles(&profiles)
        };
        Ok(ImportResult {
            profiles,
            added,
            updated,
            found,
        })
    }
}
fn public_profiles(profiles: &[StoredProfile]) -> Vec<ProfileDto> {
    let mut result: Vec<_> = profiles.iter().map(StoredProfile::dto).collect();
    // Deterministic Unicode lowercase order, independent of host locale.
    result.sort_by_key(|p| p.name.to_lowercase());
    result
}
