//! Local host inhibitor access only. This does not control a mobile screen wake lock.
use crate::{
    local::LocalClient,
    power::{PowerReport, PowerRequest},
};

impl LocalClient {
    pub async fn power_status(&self) -> anyhow::Result<PowerReport> {
        self.request(&PowerRequest::Status {}).await
    }
    pub async fn set_power(&self, active: bool) -> anyhow::Result<PowerReport> {
        self.request(&PowerRequest::Set { active }).await
    }
}
