use crate::{Error, ExpectedManifestDigest, Result};

/// An explicit trusted-source review of a closed loader contract for this exact
/// manifest. A safe probe or matching hash does not establish this policy.
///
/// The reviewer must establish scoped System32 OS imports, no attacker-selected
/// assembly/native names, and no app-local probing paths outside held payloads,
/// or use a separately proven immutable private staging policy. Never construct
/// this automatically from a manifest capability claim or a successful probe.
#[derive(Clone, Copy, Debug)]
pub struct NoDynamicCodeLoad {
    expected_manifest: ExpectedManifestDigest,
}
impl NoDynamicCodeLoad {
    pub const fn from_trusted_review(expected_manifest: ExpectedManifestDigest) -> Self {
        Self { expected_manifest }
    }
    pub(crate) fn verify(self, expected: ExpectedManifestDigest) -> Result<()> {
        if self.expected_manifest != expected {
            return Err(Error::LoaderPolicyRequired);
        }
        Ok(())
    }
}
