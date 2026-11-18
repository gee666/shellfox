use super::*;

/// Declared compatibility identity only. Authenticity is checked by the producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineIdentity(pub(super) [u8; 64]);
impl EngineIdentity {
    pub fn from_hex(text: &str) -> Result<Self> {
        if text.len() != 64
            || !text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::InvalidInput(
                "engine identity must be lowercase SHA256",
            ));
        }
        let mut bytes = [0; 64];
        bytes.copy_from_slice(text.as_bytes());
        Ok(Self(bytes))
    }
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}
/// Constructible only after validating the privately linked extension against
/// the profile identity compiled by the opt-in build. Not an authorization token.
#[derive(Debug, Clone, Copy)]
pub struct VerifiedNativeExtension {
    pub(super) identity: EngineIdentity,
}
impl VerifiedNativeExtension {
    pub fn verify() -> Result<Self> {
        let expected = EngineIdentity::from_hex(env!("SHELLFOX_GHOSTTY_ENGINE_PROFILE"))?;
        Self::verify_expected(expected)
    }
    pub fn verify_expected(expected: EngineIdentity) -> Result<Self> {
        let compiled = EngineIdentity::from_hex(env!("SHELLFOX_GHOSTTY_ENGINE_PROFILE"))?;
        if expected != compiled {
            return Err(Error::IdentityMismatch);
        }
        let mut info = ExtensionInfo {
            size: std::mem::size_of::<ExtensionInfo>(),
            abi_version: 1,
            ansi_abi_version: 0,
            terminal_profile_abi_version: 0,
            snapshot_budget_abi_version: 0,
            snapshot_format_version: 0,
            features: 0,
            engine_profile: ByteString::from(&[]),
            legacy_width_profile: ByteString::from(&[]),
        };
        // SAFETY: exact Native002 C layout and initialized bounded output.
        unsafe {
            check(ghostty_shellfox_extension_info(&mut info))?;
        }
        if info.size != std::mem::size_of::<ExtensionInfo>()
            || info.abi_version != 1
            || info.ansi_abi_version != 1
            || info.terminal_profile_abi_version != 1
            || info.snapshot_budget_abi_version != 1
            || info.snapshot_format_version != 1
            || info.features != 7
        {
            return Err(Error::AbiMismatch);
        }
        let identity = read_static(info.engine_profile, 64)?;
        if identity.as_slice() != expected.0 {
            return Err(Error::IdentityMismatch);
        }
        if read_static(info.legacy_width_profile, 64)? != b"xterm-6.0.0-unicode6" {
            return Err(Error::AbiMismatch);
        }
        Ok(Self { identity: expected })
    }
    pub fn identity(&self) -> EngineIdentity {
        self.identity
    }
}
fn read_static(value: ByteString, limit: usize) -> Result<Vec<u8>> {
    if value.len > limit || (value.len > 0 && value.ptr.is_null()) {
        return Err(Error::InvalidOutput("extension string"));
    }
    let mut out = allocation(value.len, limit)?;
    if value.len != 0 {
        // SAFETY: the verified public ABI promises a static readable string.
        // Length/null validation precedes access; nothing is retained by reference.
        out.copy_from_slice(unsafe { std::slice::from_raw_parts(value.ptr, value.len) });
    }
    Ok(out)
}
