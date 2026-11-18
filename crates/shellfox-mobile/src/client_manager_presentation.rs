//! Native owner operations reuse the same checked implementation as the C ABI.
use super::*;
use shellfox_ghostty_vt::{Selection, ViewportPoint};
impl ManagerOwnerView {
    fn presentable(&self) -> Result<(), ManagerRecoveryError> {
        if self.state.lock().map_err(|_| crate::Status::Panic)?.retired {
            return Err(crate::Status::Stale.into());
        }
        Ok(())
    }
    pub fn scroll(&self, rows: i32) -> Result<(), ManagerRecoveryError> {
        self.presentable()?;
        Ok(crate::interaction::scroll(self.terminal, rows)?)
    }
    pub fn set_selection(&self, selection: Option<Selection>) -> Result<(), ManagerRecoveryError> {
        self.presentable()?;
        Ok(crate::interaction::set_selection(self.terminal, selection)?)
    }
    pub fn copy_selection(&self) -> Result<Option<String>, ManagerRecoveryError> {
        self.presentable()?;
        Ok(crate::interaction::copy_selection(self.terminal)?)
    }
    pub fn hyperlink_at(
        &self,
        point: ViewportPoint,
    ) -> Result<Option<String>, ManagerRecoveryError> {
        self.presentable()?;
        Ok(crate::interaction::hyperlink_at(self.terminal, point)?)
    }
    pub fn mouse_tracking(&self) -> Result<bool, ManagerRecoveryError> {
        self.presentable()?;
        Ok(crate::interaction::mouse_tracking(self.terminal)?)
    }
}
