//! Forward one exact native launch binding/receipt, never a privilege enum.
use super::*;
use shellfox_terminal::{NativeLaunchBinding, NativeLaunchEvidence};
impl AutomationRuns {
    pub fn start_with_launch_binding(
        &mut self,
        input: AutomationStart,
        owner: u64,
        binding: NativeLaunchBinding,
    ) -> Result<AutomationRun> {
        self.start_inner(input, owner, Some(binding))
    }
    /// Receipt survives natural raw cleanup on Run until the canonical Runtime
    /// takes it. It holds snapshot/binding data, not Job/process/token handles.
    pub fn take_launch_evidence(&mut self, id: Uuid) -> Result<Option<NativeLaunchEvidence>> {
        Ok(self
            .runs
            .get_mut(&id)
            .context("Automation launch receipt is unavailable.")?
            .launch_evidence
            .take())
    }
}
