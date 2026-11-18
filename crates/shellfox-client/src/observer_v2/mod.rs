//! Opt-in observer-v2 client. Existing crate::Client remains v1.
mod client;
mod commands;
mod connection;
mod cursor;
mod state;

pub use client::ObserverClient;
pub use connection::{DiscoveryOutcome, FallbackPolicy};
pub use cursor::{CursorError, DeliveryCursor, EventDisposition};
pub use shellfox_protocol::observer_v2 as protocol;
pub use state::{ClientError, Status};
