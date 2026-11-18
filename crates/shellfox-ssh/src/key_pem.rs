//! Container conversion from the active MIT src/main/ssh/keys.ts.
//! DER parsing, decryption and EC validation use stable RustCrypto implementations.
use crate::{key_validation, Error, Result, Secret};
use pkcs8::{der::Decode, DecodePrivateKey};
use ssh_key::{
    private::{KeypairData, RsaKeypair, RsaPrivateKey},
    public::RsaPublicKey,
    Mpint, PrivateKey,
};
use zeroize::Zeroizing;

pub(crate) fn parse(text: &str, passphrase: Option<&Secret>) -> Result<PrivateKey> {
    let (label, bytes) = pem_rfc7468::decode_vec(text.as_bytes()).map_err(|_| Error::InvalidKey)?;
    let bytes = Zeroizing::new(bytes);
    let data = if label == "ENCRYPTED PRIVATE KEY" {
        let password = passphrase.ok_or(Error::PassphraseRequired)?;
        let info = pkcs8::EncryptedPrivateKeyInfo::try_from(bytes.as_slice())
            .map_err(|_| Error::InvalidKey)?;
        let params = info
            .encryption_algorithm
            .pbes2()
            .ok_or(Error::UnsupportedKeyFormat)?;
        if let Some(p) = params.kdf.pbkdf2() {
            if p.iteration_count == 0 || p.iteration_count > 1_000_000 || p.salt.len() > 64 {
                return Err(Error::Limit);
            }
        } else if let Some(p) = params.kdf.scrypt() {
            let memory = p
                .cost_parameter
                .checked_mul(u64::from(p.block_size))
                .and_then(|n| n.checked_mul(128));
            let work = p
                .cost_parameter
                .checked_mul(u64::from(p.block_size))
                .and_then(|n| n.checked_mul(u64::from(p.parallelization)));
            if !p.cost_parameter.is_power_of_two()
                || p.block_size == 0
                || p.parallelization == 0
                || memory.is_none_or(|n| n > 256 * 1024 * 1024)
                || work.is_none_or(|n| n > 8_388_608)
                || p.salt.len() > 64
            {
                return Err(Error::Limit);
            }
        } else {
            return Err(Error::UnsupportedKeyFormat);
        }
        Zeroizing::new(
            info.decrypt(password.expose())
                .map_err(|_| Error::InvalidKey)?
                .as_bytes()
                .to_vec(),
        )
    } else {
        Zeroizing::new(bytes.to_vec())
    };
    let key = match label {
        "RSA PRIVATE KEY" => rsa(&data)?,
        "EC PRIVATE KEY" => ec_der(&data, false)?,
        "PRIVATE KEY" | "ENCRYPTED PRIVATE KEY" => pkcs8(&data)?,
        _ => return Err(Error::UnsupportedKeyFormat),
    };
    PrivateKey::new(key, "").map_err(|_| Error::InvalidKey)
}
fn pkcs8(bytes: &[u8]) -> Result<KeypairData> {
    let info = pkcs8::PrivateKeyInfo::try_from(bytes).map_err(|_| Error::InvalidKey)?;
    match info.algorithm.oid.to_string().as_str() {
        "1.2.840.113549.1.1.1" => rsa(info.private_key),
        "1.2.840.10045.2.1" => ec_der(bytes, true),
        "1.3.101.112" => Ok(KeypairData::Ed25519(
            ed25519_dalek::SigningKey::from_pkcs8_der(bytes)
                .map_err(|_| Error::InvalidKey)?
                .into(),
        )),
        _ => Err(Error::UnsupportedKeyFormat),
    }
}
fn rsa(bytes: &[u8]) -> Result<KeypairData> {
    let r = pkcs1::RsaPrivateKey::from_der(bytes).map_err(|_| Error::InvalidKey)?;
    if r.other_prime_infos.is_some() {
        return Err(Error::UnsupportedKeyFormat);
    }
    let mp = |v: &[u8]| Mpint::from_positive_bytes(v).map_err(|_| Error::InvalidKey);
    Ok(KeypairData::Rsa(RsaKeypair {
        public: RsaPublicKey {
            e: mp(r.public_exponent.as_bytes())?,
            n: mp(r.modulus.as_bytes())?,
        },
        private: RsaPrivateKey {
            d: mp(r.private_exponent.as_bytes())?,
            p: mp(r.prime1.as_bytes())?,
            q: mp(r.prime2.as_bytes())?,
            iqmp: mp(r.coefficient.as_bytes())?,
        },
    }))
}
fn ec_der(bytes: &[u8], pkcs8: bool) -> Result<KeypairData> {
    macro_rules! try_curve {
        ($module:ident, $name:literal) => {
            if let Some(key) = if pkcs8 {
                $module::SecretKey::from_pkcs8_der(bytes).ok()
            } else {
                $module::SecretKey::from_sec1_der(bytes).ok()
            } {
                let scalar = Zeroizing::new(key.to_bytes());
                return key_validation::ec($name, &scalar);
            }
        };
    }
    try_curve!(p256, "nistp256");
    try_curve!(p384, "nistp384");
    try_curve!(p521, "nistp521");
    Err(Error::InvalidKey)
}
