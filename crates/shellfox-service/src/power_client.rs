//! Local host inhibitor access only. This does not control a mobile screen wake lock.
use crate::{
    local::LocalClient,
    native::{NativeAction, NativeClient, NativeResponse},
    power::{PowerReport, PowerRequest},
};

impl NativeClient {
    pub async fn power_status(&self) -> anyhow::Result<PowerReport> {
        self.power_request(PowerRequest::Status {}).await
    }
    /// Returns confirmed state including partial capability and operation errors.
    pub async fn set_power(&self, active: bool) -> anyhow::Result<PowerReport> {
        self.power_request(PowerRequest::Set { active }).await
    }
    async fn power_request(&self, request: PowerRequest) -> anyhow::Result<PowerReport> {
        let NativeResponse::Power(report) = self.execute(NativeAction::Power(request)).await?
        else {
            anyhow::bail!("Unexpected native keep-awake response.");
        };
        shellfox_protocol::Validate::validate(&report)?;
        Ok(report)
    }
}
impl LocalClient {
    pub async fn power_status(&self) -> anyhow::Result<PowerReport> {
        self.request(&PowerRequest::Status {}).await
    }
    pub async fn set_power(&self, active: bool) -> anyhow::Result<PowerReport> {
        self.request(&PowerRequest::Set { active }).await
    }
}
