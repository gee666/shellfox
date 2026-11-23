//! Test-only loss of the original producer sender; never fake Finish/EOF/ACK.
use super::*;
impl Service {
    pub(crate) fn disconnect_automation_source_for_lifecycle_test(
        &mut self,
        id: Uuid,
    ) -> Result<()> {
        let relay = self
            .automation
            .relays
            .get_mut(&id)
            .context("Owned automation relay missing")?;
        relay.feed.disconnect_for_test();
        Ok(())
    }
}
