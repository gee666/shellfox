use crate::host_client::HostClient;
use shellfox_service::native::{NativeAction, NativeResponse};
use std::{future::Future, pin::Pin};
pub(super) type Reply<'a> =
    Pin<Box<dyn Future<Output = anyhow::Result<NativeResponse>> + Send + 'a>>;
pub(super) trait Gateway: Send + Sync {
    fn execute(&self, request: NativeAction) -> Reply<'_>;
}
impl Gateway for HostClient {
    fn execute(&self, request: NativeAction) -> Reply<'_> {
        Box::pin(HostClient::execute(self, request))
    }
}
