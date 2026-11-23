use crate::{normalize_endpoint, Error, PrivateDirectory, Result};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

const FILE: &str = "ssh-known-hosts.json";
const LOCK: &str = "ssh-known-hosts.lock";
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_HOSTS: usize = 10000;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostKeyInfo {
    #[serde(rename = "type")]
    pub key_type: String,
    pub fingerprint: String,
}
pub fn host_key_info(key: &[u8]) -> Result<HostKeyInfo> {
    if key.len() > 65536 {
        return Err(Error::Limit);
    }
    let public = ssh_key::PublicKey::from_bytes(key).map_err(|_| Error::InvalidKey)?;
    Ok(HostKeyInfo {
        key_type: public.algorithm().as_str().to_owned(),
        fingerprint: format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(key))),
    })
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrustStatus {
    Known(HostKeyInfo),
    Unknown(HostKeyInfo),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum TrustDecision {
    Reject,
    TrustAndSave,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    #[serde(deserialize_with = "unique_hosts")]
    hosts: BTreeMap<String, HostKeyInfo>,
}
fn unique_hosts<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<BTreeMap<String, HostKeyInfo>, D::Error> {
    struct Visitor;
    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = BTreeMap<String, HostKeyInfo>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("unique host keys")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut hosts = BTreeMap::new();
            while let Some((id, info)) = map.next_entry()? {
                if hosts.len() >= MAX_HOSTS || hosts.insert(id, info).is_some() {
                    return Err(serde::de::Error::custom(
                        "duplicate host or host limit exceeded",
                    ));
                }
            }
            Ok(hosts)
        }
    }
    d.deserialize_map(Visitor)
}
#[derive(Clone)]
pub struct KnownHosts {
    directory: Arc<PrivateDirectory>,
}
impl KnownHosts {
    pub fn new(directory: Arc<PrivateDirectory>) -> Self {
        Self { directory }
    }
    fn id(host: &str, port: u16) -> Result<String> {
        let (clean, user) = normalize_endpoint(host, "")?;
        if port == 0 || clean != host || !user.is_empty() {
            return Err(Error::Validation);
        }
        serde_json::to_string(&(host.to_lowercase(), port)).map_err(|_| Error::Validation)
    }
    fn load(&self) -> Result<Document> {
        let Some(bytes) = self.directory.read(FILE, MAX_BYTES)? else {
            return Ok(Document {
                version: 1,
                hosts: BTreeMap::new(),
            });
        };
        let doc: Document = serde_json::from_slice(&bytes).map_err(|_| Error::Corrupt)?;
        if doc.version != 1 || doc.hosts.len() > MAX_HOSTS {
            return Err(Error::Corrupt);
        }
        for (id, key) in &doc.hosts {
            let (host, port): (String, u16) =
                serde_json::from_str(id).map_err(|_| Error::Corrupt)?;
            if Self::id(&host, port).as_ref() != Ok(id) || !valid_info(key) {
                return Err(Error::Corrupt);
            }
        }
        Ok(doc)
    }
    fn compare(doc: &Document, id: &str, info: &HostKeyInfo) -> Result<bool> {
        match doc.hosts.get(id) {
            Some(saved) if saved != info => Err(Error::HostKeyChanged),
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }
    /// Adds the host keys Electron trusted (same document format) that this
    /// store doesn't know yet. Saved keys are never replaced; invalid rows are skipped.
    pub fn import_electron(&self, bytes: &[u8]) -> Result<usize> {
        if bytes.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        let incoming: Document = serde_json::from_slice(bytes).map_err(|_| Error::Corrupt)?;
        if incoming.version != 1 {
            return Err(Error::Corrupt);
        }
        let _lock = self.directory.lock(LOCK)?;
        let mut latest = self.load()?;
        let before = latest.hosts.len();
        for (id, info) in incoming.hosts {
            let valid = serde_json::from_str::<(String, u16)>(&id)
                .is_ok_and(|(host, port)| Self::id(&host, port).as_ref() == Ok(&id))
                && valid_info(&info);
            if valid && latest.hosts.len() < MAX_HOSTS {
                latest.hosts.entry(id).or_insert(info);
            }
        }
        let added = latest.hosts.len() - before;
        if added != 0 {
            let bytes = serde_json::to_vec_pretty(&latest).map_err(|_| Error::Storage)?;
            self.directory.atomic_write(FILE, &bytes, MAX_BYTES)?;
        }
        Ok(added)
    }
    pub fn inspect(&self, host: &str, port: u16, key: &[u8]) -> Result<TrustStatus> {
        let id = Self::id(host, port)?;
        let info = host_key_info(key)?;
        let _lock = self.directory.lock(LOCK)?;
        Ok(if Self::compare(&self.load()?, &id, &info)? {
            TrustStatus::Known(info)
        } else {
            TrustStatus::Unknown(info)
        })
    }
    /// The callback must represent an explicit local user decision after displaying host,
    /// port, type and SHA256 fingerprint. No callback is run for changed or corrupt trust.
    /// It runs outside the lock; trust is rechecked under lock before persisting approval.
    pub fn verify(
        &self,
        host: &str,
        port: u16,
        key: &[u8],
        confirm: impl FnOnce(&str, u16, &HostKeyInfo) -> Result<TrustDecision>,
    ) -> Result<()> {
        self.verify_guarded(host, port, key, confirm, || Ok(()))
    }
    pub fn verify_guarded(
        &self,
        host: &str,
        port: u16,
        key: &[u8],
        confirm: impl FnOnce(&str, u16, &HostKeyInfo) -> Result<TrustDecision>,
        check: impl Fn() -> Result<()>,
    ) -> Result<()> {
        check()?;
        let id = Self::id(host, port)?;
        let info = match self.inspect(host, port, key)? {
            TrustStatus::Known(_) => {
                check()?;
                return Ok(());
            }
            TrustStatus::Unknown(info) => info,
        };
        if confirm(host, port, &info)? != TrustDecision::TrustAndSave {
            return Err(Error::HostNotTrusted);
        }
        let _lock = self.directory.lock(LOCK)?;
        let mut latest = self.load()?;
        if Self::compare(&latest, &id, &info)? {
            check()?;
            return Ok(());
        }
        if latest.hosts.len() >= MAX_HOSTS {
            return Err(Error::Limit);
        }
        check()?;
        latest.hosts.insert(id, info);
        let bytes = serde_json::to_vec_pretty(&latest).map_err(|_| Error::Storage)?;
        self.directory.atomic_write(FILE, &bytes, MAX_BYTES)
    }
}
fn valid_info(key: &HostKeyInfo) -> bool {
    !key.key_type.is_empty()
        && key.key_type.len() <= 200
        && key
            .key_type
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"@._+-".contains(&c))
        && key
            .fingerprint
            .strip_prefix("SHA256:")
            .and_then(|s| STANDARD_NO_PAD.decode(s).ok())
            .is_some_and(|b| b.len() == 32)
}
