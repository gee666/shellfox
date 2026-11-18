use crate::Result;

/// Restriction of EXISTING native RW authority only, not a grant, cleanup fence or NoOwner proof.
/// Service's implementation retains the SAME ownership.lost token, RootLifetimeGuard and custody.
/// It synchronously denies when that exact token is cancelled. Watcher revocation is additional.
/// Sampling under a Files mutation/commit check is NOT cross-domain Go/filesystem atomicity.
pub trait NativeCommitFence: Send + Sync {
    fn check(&self) -> Result<()>;
}
