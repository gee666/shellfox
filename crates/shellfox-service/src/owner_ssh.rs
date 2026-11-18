//! Lifecycle edges for the installed native-only SSH adapter.
use crate::*;
impl Service {
    pub(crate) fn open_ssh_profile_native(
        &mut self,
        input: crate::ssh::OpenProfile,
        controller: u64,
    ) -> Result<crate::ssh::OpenedProfile> {
        let mut owner = std::mem::take(&mut self.ssh);
        let result = owner.open_profile(self, controller, input);
        self.ssh = owner;
        result
    }
    pub(crate) fn stop_ssh_confirmed(&mut self) -> Result<()> {
        self.begin_stop_ssh_native();
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            if self.stop_ssh_native()? {
                return Ok(());
            }
            self.pump();
            if Instant::now() >= deadline {
                bail!("SSH original runtime/worker/broker cleanup remains unconfirmed; owner retained.");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    pub(crate) fn close_ssh_confirmed(&mut self, tabs: &[TabId]) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            if self.close_ssh_targets(tabs)? {
                return Ok(());
            }
            self.pump();
            if Instant::now() >= deadline {
                bail!("SSH target cleanup remains unconfirmed; metadata and original owners retained.");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
