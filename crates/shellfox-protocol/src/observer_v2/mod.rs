//! Explicit observer-only v2 contract. This module does not enable a server route.
mod catalog;
mod command;
mod delivery;
mod errors;
mod facts;
mod handshake;
mod primitives;
mod replies;
mod requests;
mod session;
mod settings;
mod tab;

pub use catalog::*;
pub use command::*;
pub use delivery::*;
pub use errors::*;
pub use facts::*;
pub use handshake::*;
pub use primitives::*;
pub use replies::*;
pub use requests::*;
pub use session::*;
pub use settings::*;
pub use tab::*;

pub const PROFILE: &str = "observer-v2";
pub const MAX_RECOVERY_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_DATA_BYTES: usize = 16384;
pub const MAX_POLL_DATA_BYTES: usize = 262144;
