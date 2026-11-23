//! Trusted Host-actor persistence. No path, connection, SQL or grant escapes.
use shellfox_core::{ApprovalStoreRecord, ApprovalStoreSnapshot};
use shellfox_protocol::AppError;
use std::{future::Future, pin::Pin};
pub type ApprovalRepositoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, AppError>> + Send + 'a>>;
pub trait ApprovalRepository: Send + Sync {
    fn load(&self) -> ApprovalRepositoryFuture<'_, ApprovalStoreSnapshot>;
    fn commit(
        &self,
        expected_revision: u64,
        records: Vec<ApprovalStoreRecord>,
    ) -> ApprovalRepositoryFuture<'_, ApprovalStoreSnapshot>;
}
#[cfg(test)]
mod tests;
