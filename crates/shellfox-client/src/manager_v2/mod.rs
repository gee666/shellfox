//! Explicit full-control profile client. Default crate::Client remains interactive v1.
mod client;
mod uploads;
pub use uploads::{UploadCursor, UploadError};
mod cursor;
mod input_book;
pub use cursor::{CursorError, EventDisposition, ManagerCursor};
pub use input_book::AcceptedContext;
mod commands;
pub use crate::v2_common::{DiscoveryOutcome, FallbackPolicy, Status};
pub use client::{ManagerBindingWitness, ManagerClient};
pub type ClientError = crate::v2_common::ClientError<shellfox_protocol::manager_v2::Error>;
pub use shellfox_protocol::manager_v2 as protocol;
