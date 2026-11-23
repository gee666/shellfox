use anyhow::{bail, Result};
use std::collections::HashMap;

pub(super) fn controller_id(
    controllers: &mut HashMap<uuid::Uuid, u64>,
    service: &mut crate::Service,
    id: uuid::Uuid,
) -> Result<u64> {
    if id.is_nil() {
        bail!("A non-nil client ID is required.");
    }
    if let Some(controller) = controllers.get(&id) {
        return Ok(*controller);
    }
    if controllers.len() >= 128 {
        bail!("Service client limit reached.");
    }
    let value = service.allocate_viewer_id()?;
    controllers.insert(id, value);
    Ok(value)
}
