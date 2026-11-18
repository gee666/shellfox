use super::{PowerError, PowerLease};
pub struct Lease;
impl PowerLease for Lease {
    fn release(&mut self) -> Result<(), PowerError> {
        Ok(())
    }
}
pub fn acquire() -> Result<Lease, PowerError> {
    Err(PowerError(
        "Keep awake is not supported on this platform.".into(),
    ))
}
