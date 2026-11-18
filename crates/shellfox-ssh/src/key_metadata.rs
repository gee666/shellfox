//! Compatibility inventory is separate from cryptographic validation or permission to connect.
use crate::{keys::MAX_KEY_BYTES, Error, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum KeyFormat {
    OpenSsh,
    Putty2,
    Putty3,
    Pkcs8,
    TraditionalPem,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeyMetadata {
    pub format: KeyFormat,
    pub encrypted: bool,
    pub algorithm: Option<String>,
    pub limitation: Option<String>,
}
/// Structural format inspection only. In particular, metadata for an encrypted
/// legacy key is NOT a statement that its password/MAC/private components validate.
pub fn inspect(data: &[u8]) -> Result<KeyMetadata> {
    if data.len() > MAX_KEY_BYTES {
        return Err(Error::Limit);
    }
    let text = std::str::from_utf8(data)
        .map_err(|_| Error::InvalidKey)?
        .trim();
    if let Some(header) = text
        .lines()
        .next()
        .filter(|l| l.starts_with("PuTTY-User-Key-File-"))
    {
        let (prefix, algorithm) = header.split_once(": ").ok_or(Error::InvalidKey)?;
        let format = match prefix {
            "PuTTY-User-Key-File-2" => KeyFormat::Putty2,
            "PuTTY-User-Key-File-3" => KeyFormat::Putty3,
            _ => return Err(Error::InvalidKey),
        };
        if algorithm.len() > 200
            || !algorithm
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"@._+-".contains(&c))
        {
            return Err(Error::InvalidKey);
        }
        let encrypted = text
            .lines()
            .find_map(|l| l.strip_prefix("Encryption: "))
            .ok_or(Error::InvalidKey)?
            != "none";
        return Ok(KeyMetadata{format,encrypted,algorithm:Some(algorithm.into()),limitation:(algorithm=="ssh-dss").then(||"Legacy DSA requires an explicit compatibility policy; modern OpenSSH may not implement it.".into())});
    }
    let labels = [
        "OPENSSH PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
        "PRIVATE KEY",
        "RSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "DSA PRIVATE KEY",
    ];
    let label = labels
        .into_iter()
        .find(|label| text.starts_with(&format!("-----BEGIN {label}-----")))
        .ok_or(Error::InvalidKey)?;
    let body = text
        .strip_prefix(&format!("-----BEGIN {label}-----"))
        .and_then(|s| s.strip_suffix(&format!("-----END {label}-----")))
        .ok_or(Error::InvalidKey)?;
    let mut encoded = Zeroizing::new(String::new());
    let mut traditional_encryption = false;
    let mut dek_info = false;
    for line in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line == "Proc-Type: 4,ENCRYPTED" {
            if traditional_encryption {
                return Err(Error::InvalidKey);
            }
            traditional_encryption = true;
            continue;
        }
        if let Some(info) = line.strip_prefix("DEK-Info: ") {
            if dek_info {
                return Err(Error::InvalidKey);
            }
            dek_info = true;
            let (cipher, iv) = info.split_once(',').ok_or(Error::InvalidKey)?;
            if ![
                "AES-128-CBC",
                "AES-192-CBC",
                "AES-256-CBC",
                "DES-EDE3-CBC",
                "DES-CBC",
            ]
            .contains(&cipher)
                || ![16, 32].contains(&iv.len())
                || !iv.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(Error::InvalidKey);
            }
            continue;
        }
        encoded.push_str(line);
    }
    if traditional_encryption != dek_info
        || (dek_info && !["RSA PRIVATE KEY", "EC PRIVATE KEY", "DSA PRIVATE KEY"].contains(&label))
    {
        return Err(Error::InvalidKey);
    }
    let decoded = Zeroizing::new(
        STANDARD
            .decode(encoded.as_bytes())
            .map_err(|_| Error::InvalidKey)?,
    );
    if decoded.is_empty() {
        return Err(Error::InvalidKey);
    }
    let algorithm = match label {
        "RSA PRIVATE KEY" => Some("ssh-rsa"),
        "DSA PRIVATE KEY" => Some("ssh-dss"),
        _ => None,
    }
    .map(str::to_owned);
    let algorithm = if label == "PRIVATE KEY" {
        pkcs8::PrivateKeyInfo::try_from(decoded.as_slice())
            .ok()
            .and_then(|info| {
                (info.algorithm.oid.to_string() == "1.2.840.10040.4.1").then(|| "ssh-dss".into())
            })
    } else {
        algorithm
    };
    let format = match label {
        "OPENSSH PRIVATE KEY" => KeyFormat::OpenSsh,
        "ENCRYPTED PRIVATE KEY" | "PRIVATE KEY" => KeyFormat::Pkcs8,
        _ => KeyFormat::TraditionalPem,
    };
    let encrypted = traditional_encryption
        || label == "ENCRYPTED PRIVATE KEY"
        || (label == "OPENSSH PRIVATE KEY" && decoded.get(15..23) != Some(b"\0\0\0\x04none"));
    let limitation = if traditional_encryption {
        Some("Traditional encrypted PEM is preserved; its legacy decryption adapter still needs review and an explicit compatibility policy.".into())
    } else if algorithm.as_deref() == Some("ssh-dss") {
        Some("Legacy DSA is preserved; enabling obsolete signatures requires a user policy decision.".into())
    } else {
        None
    };
    Ok(KeyMetadata {
        format,
        encrypted,
        algorithm,
        limitation,
    })
}
