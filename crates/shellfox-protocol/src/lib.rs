//! Existing Shellfox v1 outbound contracts. No local host, filesystem or PTY access.
#[cfg(feature = "local-host")]
pub mod automation;
#[cfg(feature = "local-host")]
mod local;
#[cfg(feature = "local-host")]
pub use local::*;
mod assets;
mod chunks;
mod directories;
mod error;
pub mod files;
mod ids;
mod manager;
mod methods;
mod probe;
mod remote_commands;
mod session_requests;
mod settings;
mod stats;
mod terminal;
mod terminal_requests;
mod transfer_requests;
mod transfer_responses;
mod transport;
mod upload_requests;
pub mod validation;
mod wire;

pub use assets::*;
pub use chunks::*;
pub use directories::*;
pub use error::*;
pub use manager::*;
pub use methods::*;
pub use probe::*;
pub use remote_commands::*;
pub use session_requests::*;
pub use settings::*;
pub use shellfox_core::{EnvVar, Generation, SessionId, TabId};
pub use stats::*;
pub use terminal::*;
pub use terminal_requests::*;
pub use transfer_requests::*;
pub use transfer_responses::*;
pub use transport::*;
pub use upload_requests::*;
pub use uuid::Uuid as ClientId;
pub use wire::*;

pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Validation runs after decoding and before sending. Serde rejects unknown fields.
pub trait Validate {
    fn validate(&self) -> Result<(), ValidationError>;
}
