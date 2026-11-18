use crate::{Error, Result};
use num_bigint_dig::BigUint;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use ring::{
    rand::SystemRandom,
    rsa::{KeyPair, KeyPairComponents, PublicKeyComponents},
    signature::RSA_PKCS1_SHA256,
};
use ssh_key::private::{EcdsaKeypair, KeypairData, RsaKeypair};
use zeroize::Zeroizing;

pub(crate) fn ec(curve: &str, scalar: &[u8]) -> Result<KeypairData> {
    macro_rules! key {
        ($module:ident, $size:literal, $variant:ident) => {{
            if scalar.len() > $size {
                return Err(Error::InvalidKey);
            }
            let mut bytes = Zeroizing::new([0u8; $size]);
            bytes[$size - scalar.len()..].copy_from_slice(scalar);
            let secret =
                $module::SecretKey::from_slice(bytes.as_ref()).map_err(|_| Error::InvalidKey)?;
            EcdsaKeypair::$variant {
                public: secret.public_key().to_encoded_point(false),
                private: secret.into(),
            }
        }};
    }
    Ok(KeypairData::Ecdsa(match curve {
        "nistp256" => key!(p256, 32, NistP256),
        "nistp384" => key!(p384, 48, NistP384),
        "nistp521" => key!(p521, 66, NistP521),
        _ => return Err(Error::InvalidKey),
    }))
}

/// ring does private RSA operations, not RustCrypto rsa, which has RUSTSEC-2023-0071.
/// Component conversion matches the existing Node JWK import. No new RSA construction.
pub(crate) fn rsa(key: &RsaKeypair) -> Result<()> {
    let bytes = |m: &ssh_key::Mpint| {
        m.as_positive_bytes()
            .ok_or(Error::InvalidKey)
            .map(|s| Zeroizing::new(s.to_vec()))
    };
    let (n, e, d, p, q, iqmp) = (
        bytes(&key.public.n)?,
        bytes(&key.public.e)?,
        bytes(&key.private.d)?,
        bytes(&key.private.p)?,
        bytes(&key.private.q)?,
        bytes(&key.private.iqmp)?,
    );
    if n.len() > 512 || e.len() > 5 || d.len() > 512 || p.len() > 256 || q.len() > 256 {
        return Err(Error::Limit);
    }
    let big_d = Zeroizing::new(BigUint::from_bytes_be(&d));
    let big_p = Zeroizing::new(BigUint::from_bytes_be(&p));
    let big_q = Zeroizing::new(BigUint::from_bytes_be(&q));
    if *big_p <= BigUint::from(1u8) || *big_q <= BigUint::from(1u8) {
        return Err(Error::InvalidKey);
    }
    let pm1 = Zeroizing::new(&*big_p - 1u8);
    let qm1 = Zeroizing::new(&*big_q - 1u8);
    let big_dp = Zeroizing::new(&*big_d % &*pm1);
    let big_dq = Zeroizing::new(&*big_d % &*qm1);
    let dp = Zeroizing::new(big_dp.to_bytes_be());
    let dq = Zeroizing::new(big_dq.to_bytes_be());
    let pair = KeyPair::from_components(&KeyPairComponents {
        public_key: PublicKeyComponents {
            n: n.as_slice(),
            e: e.as_slice(),
        },
        d: d.as_slice(),
        p: p.as_slice(),
        q: q.as_slice(),
        dP: dp.as_slice(),
        dQ: dq.as_slice(),
        qInv: iqmp.as_slice(),
    })
    .map_err(|_| Error::InvalidKey)?;
    let mut signature = Zeroizing::new(vec![0; pair.public().modulus_len()]);
    pair.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        b"Shellfox offline RSA import consistency check",
        &mut signature,
    )
    .map_err(|_| Error::InvalidKey)
}
