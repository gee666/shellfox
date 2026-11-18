//! OpenSSH, PuTTY v2/v3 and PKCS8 import. Decrypted keys never reach disk.
use crate::{host_key_info, Error, HostKeyInfo, PrivateDirectory, Result, Secret};
use base64::{engine::general_purpose::STANDARD, Engine};
use ssh_key::{private::KeypairData, public::KeyData, HashAlg, Kdf, PrivateKey};
use std::{fmt, path::Path, sync::Arc};
use uuid::Uuid;
use zeroize::Zeroizing;

pub const MAX_KEY_BYTES: usize = 128 * 1024;
pub struct ValidatedKey(PrivateKey);
impl fmt::Debug for ValidatedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ValidatedKey([REDACTED])")
    }
}
impl ValidatedKey {
    pub fn public_blob(&self) -> Result<Vec<u8>> {
        self.0
            .public_key()
            .to_bytes()
            .map_err(|_| Error::InvalidKey)
    }
    pub fn info(&self) -> Result<HostKeyInfo> {
        host_key_info(&self.public_blob()?)
    }
    /// Host connection adapter only. No RPC serialization is implemented.
    pub fn private_key(&self) -> &PrivateKey {
        &self.0
    }
    pub fn to_openssh_in_memory(&self) -> Result<Zeroizing<String>> {
        self.0
            .to_openssh(ssh_key::LineEnding::LF)
            .map_err(|_| Error::InvalidKey)
    }
}
pub fn parse_private_key(data: &[u8], passphrase: Option<&Secret>) -> Result<ValidatedKey> {
    if data.len() > MAX_KEY_BYTES {
        return Err(Error::Limit);
    }
    let text = std::str::from_utf8(data).map_err(|_| Error::InvalidKey)?;
    let key = if text.starts_with("PuTTY-User-Key-File-") {
        crate::key_ppk::parse(text, passphrase)?
    } else if text.contains("-----BEGIN OPENSSH PRIVATE KEY-----") {
        // ssh-key 0.6's PEM decoder hardcodes 70-column wrapping. ssh2 writes 64.
        // Decode the same bounded container without assuming a wrapping width.
        let body = text
            .trim()
            .strip_prefix("-----BEGIN OPENSSH PRIVATE KEY-----")
            .and_then(|s| s.strip_suffix("-----END OPENSSH PRIVATE KEY-----"))
            .ok_or(Error::InvalidKey)?;
        let encoded = Zeroizing::new(body.lines().map(str::trim).collect::<String>());
        let binary = Zeroizing::new(
            STANDARD
                .decode(encoded.as_bytes())
                .map_err(|_| Error::InvalidKey)?,
        );
        PrivateKey::from_bytes(&binary).map_err(|_| Error::InvalidKey)?
    } else {
        crate::key_pem::parse(text, passphrase)?
    };
    if let Kdf::Bcrypt { salt, rounds } = key.kdf() {
        if *rounds == 0 || *rounds > 128 || salt.len() > 64 {
            return Err(Error::Limit);
        }
    }
    // Cap the work before decryption or a private operation.
    if let KeyData::Rsa(public) = key.public_key().key_data() {
        if public.n.as_bytes().len() > 513 || public.e.as_bytes().len() > 6 {
            return Err(Error::Limit);
        }
    }
    let key = if key.is_encrypted() {
        let password = passphrase.ok_or(Error::PassphraseRequired)?;
        key.decrypt(password.expose())
            .map_err(|_| Error::InvalidKey)?
    } else {
        key
    };
    match key.key_data() {
        KeypairData::Rsa(rsa) => {
            crate::key_validation::rsa(rsa)?;
            return Ok(ValidatedKey(key));
        }
        KeypairData::Ed25519(_) | KeypairData::Ecdsa(_) => {}
        _ => return Err(Error::UnsupportedKeyFormat),
    }
    // Prove private/public consistency, rather than accepting a container header alone.
    let message = b"Shellfox offline imported-key consistency check";
    let signature = key
        .sign("shellfox-key-import", HashAlg::Sha256, message)
        .map_err(|_| Error::InvalidKey)?;
    key.public_key()
        .verify("shellfox-key-import", message, &signature)
        .map_err(|_| Error::InvalidKey)?;
    Ok(ValidatedKey(key))
}
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportedKey {
    pub id: Uuid,
    pub info: HostKeyInfo,
}
/// Host-only encrypted temporary identity. No Serialize or Clone implementation.
pub struct PreparedIdentity {
    path: std::path::PathBuf,
    file: String,
    passphrase: Secret,
    directory: Arc<PrivateDirectory>,
}
impl PreparedIdentity {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn passphrase(&self) -> &Secret {
        &self.passphrase
    }
    /// Call after the owned SSH process tree has stopped. On failure retain this object.
    pub fn cleanup(&self) -> Result<()> {
        self.directory.remove(&self.file)
    }
}
impl fmt::Debug for PreparedIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PreparedIdentity([HOST-ONLY])")
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedMaterial {
    pub id: Uuid,
    pub metadata: crate::key_metadata::KeyMetadata,
    pub cryptographically_validated: bool,
    pub info: Option<HostKeyInfo>,
}
pub struct KeyStore {
    directory: Arc<PrivateDirectory>,
}
impl KeyStore {
    pub fn new(directory: Arc<PrivateDirectory>) -> Self {
        Self { directory }
    }
    /// Preserve explicitly chosen legacy material and its limitation metadata without
    /// pretending it validates or can connect. Normal imports still require validation.
    pub fn import_preserving_legacy(
        &self,
        original: &[u8],
        passphrase: Option<&Secret>,
    ) -> Result<ImportedMaterial> {
        let mut metadata = crate::key_metadata::inspect(original)?;
        let parsed = parse_private_key(original, passphrase);
        let (validated, info) = match parsed {
            Ok(key) => (true, Some(key.info()?)),
            Err(Error::UnsupportedKeyFormat) => {
                if metadata.limitation.is_none() {
                    metadata.limitation=Some("This parsed container is preserved; its private algorithm or cipher needs an explicit compatibility review.".into());
                }
                (false, None)
            }
            Err(Error::PassphraseRequired) if metadata.limitation.is_some() => (false, None),
            Err(Error::InvalidKey)
                if metadata.format == crate::key_metadata::KeyFormat::TraditionalPem
                    && metadata.encrypted
                    && metadata.limitation.is_some() =>
            {
                (false, None)
            }
            Err(error) => return Err(error),
        };
        let result = ImportedMaterial {
            id: Uuid::new_v4(),
            metadata,
            cryptographically_validated: validated,
            info,
        };
        let file = Self::file_name(result.id);
        self.directory
            .atomic_write(&file, original, MAX_KEY_BYTES)?;
        let record = serde_json::to_vec(&result).map_err(|_| Error::Storage)?;
        if let Err(error) =
            self.directory
                .atomic_write(&format!("key-{}.metadata", result.id), &record, 4096)
        {
            let _ = self.directory.remove(&file);
            return Err(error);
        }
        Ok(result)
    }
    pub fn metadata(&self, id: Uuid) -> Result<Option<ImportedMaterial>> {
        self.directory
            .read(&format!("key-{id}.metadata"), 4096)?
            .map(|bytes| serde_json::from_slice(&bytes).map_err(|_| Error::Corrupt))
            .transpose()
    }
    pub fn import(&self, original: &[u8], passphrase: Option<&Secret>) -> Result<ImportedKey> {
        let key = parse_private_key(original, passphrase)?;
        let result = ImportedKey {
            id: Uuid::new_v4(),
            info: key.info()?,
        };
        // Preserve encryption. Never persist the decrypted representation.
        self.directory
            .atomic_write(&Self::file_name(result.id), original, MAX_KEY_BYTES)?;
        Ok(result)
    }
    pub fn load(&self, id: Uuid, passphrase: Option<&Secret>) -> Result<ValidatedKey> {
        let bytes = self
            .directory
            .read(&Self::file_name(id), MAX_KEY_BYTES)?
            .ok_or(Error::InvalidKey)?;
        parse_private_key(&bytes, passphrase)
    }
    /// Re-encrypt for retained OpenSSH without ever writing a decrypted key.
    /// The caller must keep the result alive with its owned SSH process and broker
    /// the passphrase privately. Never put it in argv, logs or the environment.
    pub fn prepare_for_system(
        &self,
        id: Uuid,
        unlock: Option<&Secret>,
        rng: &mut impl ssh_key::rand_core::CryptoRngCore,
    ) -> Result<PreparedIdentity> {
        let key = self.load(id, unlock)?;
        self.prepare_validated(key, rng)
    }
    pub fn prepare_bytes_for_system(
        &self,
        bytes: &[u8],
        unlock: Option<&Secret>,
        rng: &mut impl ssh_key::rand_core::CryptoRngCore,
    ) -> Result<PreparedIdentity> {
        self.prepare_validated(parse_private_key(bytes, unlock)?, rng)
    }
    fn prepare_validated(
        &self,
        key: ValidatedKey,
        rng: &mut impl ssh_key::rand_core::CryptoRngCore,
    ) -> Result<PreparedIdentity> {
        let mut random = Zeroizing::new([0u8; 32]);
        rng.try_fill_bytes(random.as_mut())
            .map_err(|_| Error::Process)?;
        let password =
            Secret::new(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random.as_ref()))?;
        let encoded = key
            .private_key()
            .encrypt(rng, password.expose())
            .map_err(|_| Error::InvalidKey)?
            .to_openssh(ssh_key::LineEnding::LF)
            .map_err(|_| Error::InvalidKey)?;
        let file = format!("identity-{}.key", Uuid::new_v4());
        self.directory
            .atomic_write(&file, encoded.as_bytes(), MAX_KEY_BYTES)?;
        Ok(PreparedIdentity {
            path: self.directory.file_path(&file)?,
            file,
            passphrase: password,
            directory: self.directory.clone(),
        })
    }
    pub fn file_name(id: Uuid) -> String {
        format!("key-{id}.key")
    }
}
