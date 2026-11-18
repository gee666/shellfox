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
mod manager_connection;
pub mod manager_policy;
mod observer_connection;
pub mod observer_policy;
mod policy;
mod store;
pub mod terminal_upload;
pub use manager_connection::{
    ManagerConnection, ManagerFailure, ManagerOperation, ManagerOwnerRequest,
};
pub use observer_connection::{ObserverConnection, ObserverOperation, ObserverOwnerRequest};
#[cfg(test)]
mod test_support;
mod transfers;
pub use transfers::RemoteTransfers;

pub use approval_repository::{ApprovalRepository, ApprovalRepositoryFuture};
pub use approvals::{Actor, ApprovalControl, ApprovalState, PendingPeer};
pub use backend::{Backend, BackendHandle, ManagerHttpReply, ObserverHttpReply};
pub use channel::{
    owner_channel, OwnerClient, OwnerCommand, OwnerReceiver, OwnerRequest, RemoteHost, ServiceHost,
};
pub use policy::{Approval, ViewPermission};
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
