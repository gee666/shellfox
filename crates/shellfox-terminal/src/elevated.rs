//! Retained administrator ConPTY helper boundary, separate from local PTYs.
//! The helper owns UAC authentication and the elevated Job. A launcher exit or
//! a PID is never elevated cleanup evidence. Isolated mode requires a verified
//! full bundle, explicit loader review and a fresh user approval action.
mod channel;
#[cfg(any(test, feature = "test-support"))]
mod fixture;
mod framing;
#[cfg(windows)]
mod isolated_env;
mod launch;
#[cfg(windows)]
mod native;
#[cfg(any(test, feature = "test-support"))]
mod resource;
#[cfg(any(windows, test, feature = "test-support"))]
mod session;
#[cfg(any(windows, test, feature = "test-support"))]
mod stdio;
#[cfg(any(windows, test, feature = "test-support"))]
mod stdio_workers;
#[cfg(test)]
mod tests;
mod transport;
mod wire;

pub use channel::{Channel, Event, Generation, Phase, StartupError};
#[cfg(any(test, feature = "test-support"))]
pub use fixture::{fixture_session, FixtureScenario};
pub use framing::LineDecoder;
pub use launch::{Capability, Launch, PreparedLaunch};
#[cfg(windows)]
pub use native::{NativeFactory, UserApproval};
#[cfg(any(test, feature = "test-support"))]
pub use resource::{TrustedHash, VerifiedHelper};
#[cfg(any(windows, test, feature = "test-support"))]
pub use session::{Session, StartError};
#[cfg(any(windows, test, feature = "test-support"))]
pub use stdio::{reap_retained_launchers, Observation};
pub use transport::{HelperFrame, HelperIo};
pub use wire::{AdapterError, Birth, ProcessIdentity, MAX_FRAME, MAX_INPUT};
