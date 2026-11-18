//! Port of MIT src/main/ssh/keys.ts PuTTY v2/v3 container conversion.
//! HMAC authenticates comments and private bytes before any private-key decoding.
use crate::{key_validation, Error, Result, Secret};
use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use ssh_key::{
    private::{Ed25519Keypair, KeypairData, RsaKeypair, RsaPrivateKey},
    public::RsaPublicKey,
    Mpint, PrivateKey,
};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

pub(crate) fn parse(text: &str, passphrase: Option<&Secret>) -> Result<PrivateKey> {
    let mut lines = text.trim().lines();
    let header = lines.next().ok_or(Error::InvalidKey)?;
    let (version, kind) = if let Some(s) = header.strip_prefix("PuTTY-User-Key-File-2: ") {
        (2, s)
    } else if let Some(s) = header.strip_prefix("PuTTY-User-Key-File-3: ") {
        (3, s)
    } else {
        return Err(Error::InvalidKey);
    };
    let mut fields = BTreeMap::new();
    let mut blobs = BTreeMap::new();
    while let Some(line) = lines.next() {
        let (name, value) = line.split_once(": ").ok_or(Error::InvalidKey)?;
        if name == "Public-Lines" || name == "Private-Lines" {
            let count: usize = value.parse().map_err(|_| Error::InvalidKey)?;
            if count == 0 || count > 10000 {
                return Err(Error::Limit);
            }
            let mut encoded = Zeroizing::new(String::new());
            for _ in 0..count {
                encoded.push_str(lines.next().ok_or(Error::InvalidKey)?);
            }
            let data = Zeroizing::new(
                STANDARD
                    .decode(encoded.as_bytes())
                    .map_err(|_| Error::InvalidKey)?,
            );
            if blobs.insert(name, data).is_some() {
                return Err(Error::InvalidKey);
            }
        } else if fields.insert(name, value).is_some() {
            return Err(Error::InvalidKey);
        }
    }
    let get = |name| fields.get(name).copied().ok_or(Error::InvalidKey);
    let public = blobs.remove("Public-Lines").ok_or(Error::InvalidKey)?;
    let mut private = blobs.remove("Private-Lines").ok_or(Error::InvalidKey)?;
    let encryption = get("Encryption")?;
    let encrypted = match encryption {
        "none" => false,
        "aes256-cbc" => true,
        _ => return Err(Error::UnsupportedKeyFormat),
    };
    let password = if encrypted {
        passphrase.ok_or(Error::PassphraseRequired)?.expose()
    } else {
        ""
    };
    let mut mac_key = Zeroizing::new(Vec::new());
    if encrypted {
        if !private.len().is_multiple_of(16) {
            return Err(Error::InvalidKey);
        }
        let mut derived = Zeroizing::new(vec![0; 80]);
        if version == 2 {
            for i in 0u32..2 {
                let mut hash = Sha1::new();
                hash.update(i.to_be_bytes());
                hash.update(password.as_bytes());
                derived[i as usize * 20..i as usize * 20 + 20].copy_from_slice(&hash.finalize());
            }
            cbc::Decryptor::<aes::Aes256>::new_from_slices(&derived[..32], &[0; 16])
                .map_err(|_| Error::InvalidKey)?
                .decrypt_padded_mut::<NoPadding>(&mut private)
                .map_err(|_| Error::InvalidKey)?;
        } else {
            let number =
                |name| -> Result<u32> { get(name)?.parse().map_err(|_| Error::InvalidKey) };
            let memory = number("Argon2-Memory")?;
            let passes = number("Argon2-Passes")?;
            let parallel = number("Argon2-Parallelism")?;
            if !(8..=262144).contains(&memory)
                || !(1..=100).contains(&passes)
                || !(1..=64).contains(&parallel)
                || u64::from(memory) * u64::from(passes) > 1_048_576
            {
                return Err(Error::Limit);
            }
            let algorithm = match get("Key-Derivation")? {
                "Argon2id" => Algorithm::Argon2id,
                "Argon2i" => Algorithm::Argon2i,
                "Argon2d" => Algorithm::Argon2d,
                _ => return Err(Error::InvalidKey),
            };
            let salt = hex(get("Argon2-Salt")?)?;
            if !(8..=64).contains(&salt.len()) {
                return Err(Error::Limit);
            }
            let params =
                Params::new(memory, passes, parallel, Some(80)).map_err(|_| Error::InvalidKey)?;
            Argon2::new(algorithm, Version::V0x13, params)
                .hash_password_into(password.as_bytes(), &salt, &mut derived)
                .map_err(|_| Error::InvalidKey)?;
            cbc::Decryptor::<aes::Aes256>::new_from_slices(&derived[..32], &derived[32..48])
                .map_err(|_| Error::InvalidKey)?
                .decrypt_padded_mut::<NoPadding>(&mut private)
                .map_err(|_| Error::InvalidKey)?;
            mac_key.extend_from_slice(&derived[48..80]);
        }
    }
    if version == 2 {
        let mut hash = Sha1::new();
        hash.update(b"putty-private-key-file-mac-key");
        hash.update(password.as_bytes());
        mac_key.extend_from_slice(&hash.finalize());
    }
    let mut authenticated = Zeroizing::new(Vec::new());
    for bytes in [
        kind.as_bytes(),
        encryption.as_bytes(),
        get("Comment")?.as_bytes(),
        &public,
        &private,
    ] {
        authenticated.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        authenticated.extend_from_slice(bytes);
    }
    let expected = hex(get("Private-MAC")?)?;
    macro_rules! verify {
        ($hash:ty) => {{
            let mut mac = Hmac::<$hash>::new_from_slice(&mac_key).map_err(|_| Error::InvalidKey)?;
            mac.update(&authenticated);
            mac.verify_slice(&expected).map_err(|_| Error::InvalidKey)?;
        }};
    }
    if version == 2 {
        verify!(Sha1);
    } else {
        verify!(Sha256);
    }
    let mut pub_reader = Reader(&public);
    if pub_reader.string()? != kind.as_bytes() {
        return Err(Error::InvalidKey);
    }
    let mut priv_reader = Reader(&private);
    let data = match kind {
        "ssh-rsa" => {
            let (e, n) = (mp(pub_reader.string()?)?, mp(pub_reader.string()?)?);
            let (d, p, q, iqmp) = (
                mp(priv_reader.string()?)?,
                mp(priv_reader.string()?)?,
                mp(priv_reader.string()?)?,
                mp(priv_reader.string()?)?,
            );
            KeypairData::Rsa(RsaKeypair {
                public: RsaPublicKey { e, n },
                private: RsaPrivateKey { d, p, q, iqmp },
            })
        }
        "ssh-ed25519" => {
            let public_bytes = pub_reader.string()?;
            let seed = positive(priv_reader.string()?);
            if seed.len() > 32 {
                return Err(Error::InvalidKey);
            }
            let mut fixed = Zeroizing::new([0; 32]);
            fixed[32 - seed.len()..].copy_from_slice(seed);
            let key = Ed25519Keypair::from_seed(&fixed);
            if key.public.as_ref() != public_bytes {
                return Err(Error::InvalidKey);
            }
            KeypairData::Ed25519(key)
        }
        "ecdsa-sha2-nistp256" | "ecdsa-sha2-nistp384" | "ecdsa-sha2-nistp521" => {
            let curve = std::str::from_utf8(pub_reader.string()?).map_err(|_| Error::InvalidKey)?;
            if kind != format!("ecdsa-sha2-{curve}") {
                return Err(Error::InvalidKey);
            }
            let point = pub_reader.string()?;
            let data = key_validation::ec(curve, positive(priv_reader.string()?))?;
            if let KeypairData::Ecdsa(key) = &data {
                if key.public_key_bytes() != point {
                    return Err(Error::InvalidKey);
                }
            }
            data
        }
        _ => return Err(Error::UnsupportedKeyFormat),
    };
    if !pub_reader.0.is_empty() {
        return Err(Error::InvalidKey);
    }
    if (!encrypted && !priv_reader.0.is_empty()) || priv_reader.0.len() > 32 {
        return Err(Error::InvalidKey);
    }
    let key = PrivateKey::new(data, get("Comment")?).map_err(|_| Error::InvalidKey)?;
    if key
        .public_key()
        .to_bytes()
        .map_err(|_| Error::InvalidKey)?
        .as_slice()
        != public.as_slice()
    {
        return Err(Error::InvalidKey);
    }
    Ok(key)
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn string(&mut self) -> Result<&'a [u8]> {
        let size = self.0.get(..4).ok_or(Error::InvalidKey)?;
        let n = u32::from_be_bytes(size.try_into().map_err(|_| Error::InvalidKey)?) as usize;
        let bytes = self
            .0
            .get(4..4usize.checked_add(n).ok_or(Error::InvalidKey)?)
            .ok_or(Error::InvalidKey)?;
        self.0 = &self.0[4 + n..];
        Ok(bytes)
    }
}
fn positive(mut bytes: &[u8]) -> &[u8] {
    while bytes.first() == Some(&0) {
        bytes = &bytes[1..];
    }
    bytes
}
fn mp(bytes: &[u8]) -> Result<Mpint> {
    Mpint::from_positive_bytes(positive(bytes)).map_err(|_| Error::InvalidKey)
}
fn hex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) || s.len() > 128 {
        return Err(Error::InvalidKey);
    }
    s.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let a = (pair[0] as char).to_digit(16).ok_or(Error::InvalidKey)?;
            let b = (pair[1] as char).to_digit(16).ok_or(Error::InvalidKey)?;
            Ok((a * 16 + b) as u8)
        })
        .collect()
}
