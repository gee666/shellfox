//! Authenticated retained-helper backend. The HTTP task never owns a Host or PTY.
pub mod adapter;
mod approval_async;
mod approval_records;
mod approval_repository;
mod approvals;
pub mod assets;
mod backend;
mod channel;
pub mod directories;
#[cfg(test)]
mod file_adapter_tests;
pub mod file_ops;
mod http;
mod policy;
mod poll_wake;
mod store;
pub mod terminal_upload;
#[cfg(test)]
mod test_support;
pub mod transfer_bridge;
mod transfers;
pub use transfers::RemoteTransfers;

pub use approval_repository::{ApprovalRepository, ApprovalRepositoryFuture};
pub use approvals::{Actor, ApprovalControl, ApprovalState, PendingPeer};
pub use backend::{Backend, BackendHandle};
pub use channel::{
    owner_channel, owner_channel_with_wake, OwnerClient, OwnerCommand, OwnerReceiver, OwnerRequest,
    RemoteHost, ServiceHost,
};
pub use policy::{Approval, ViewPermission};
pub use poll_wake::PollWake;
use shellfox_protocol::{AppError, ErrorCode};
pub use store::PrivateApprovalStore;

pub(crate) fn error(code: ErrorCode, message: &str, retryable: bool) -> AppError {
    AppError {
        code,
        message: message.into(),
        retryable,
    }
}
pub fn capability_unavailable(capability: &str) -> AppError {
    error(
        ErrorCode::Unsupported,
        &format!("Capability unavailable: {capability}."),
        false,
    )
}
