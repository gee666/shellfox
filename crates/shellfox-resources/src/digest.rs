use crate::{Error, Result};

/// Supplied by a trusted build record or a compiled package anchor. Never infer
/// this value by hashing a manifest obtained from an untrusted bundle directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpectedManifestDigest([u8; 32]);

pub type TrustedManifestDigest = ExpectedManifestDigest;

impl ExpectedManifestDigest {
    pub const fn from_trusted_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub fn from_trusted_hex(value: &str) -> Result<Self> {
        Ok(Self(parse_hex(value)?))
    }
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Optional anchors explicitly supplied to the trusted Cargo build. No package
/// directory or manifest is hashed at runtime to manufacture these values.
pub fn compiled_manifest_anchor(
    id: crate::TrustedBundleId,
) -> Result<Option<ExpectedManifestDigest>> {
    let value = match id {
        crate::TrustedBundleId::WindowsManagedHelper => {
            option_env!("SHELLFOX_WINDOWS_HELPER_MANIFEST_SHA256")
        }
        crate::TrustedBundleId::GoTailscale => option_env!("SHELLFOX_GO_HELPER_MANIFEST_SHA256"),
        crate::TrustedBundleId::DarwinTerminal => {
            option_env!("SHELLFOX_DARWIN_HELPERS_MANIFEST_SHA256")
        }
        crate::TrustedBundleId::WslSupervisor => {
            option_env!("SHELLFOX_WSL_SCRIPTS_MANIFEST_SHA256")
        }
    };
    value
        .filter(|value| !value.is_empty())
        .map(ExpectedManifestDigest::from_trusted_hex)
        .transpose()
}

pub(crate) fn parse_hex(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 {
        return Err(Error::Manifest("SHA-256 must be 64 lowercase hex digits"));
    }
    let mut result = [0; 32];
    for (output, pair) in result.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        let digit = |byte| match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            _ => Err(Error::Manifest("SHA-256 must be lowercase hex")),
        };
        *output = digit(pair[0])? * 16 + digit(pair[1])?;
    }
    Ok(result)
}
