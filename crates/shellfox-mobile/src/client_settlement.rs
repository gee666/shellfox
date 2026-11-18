use super::*;

/// Remember events while an attachment snapshot is replacing the cursor. Keep
/// generations separate so a late old-generation event cannot erase a newer one.
pub(crate) fn record_settlement(binding: &mut Binding, event: &TerminalEvent) -> Result<bool> {
    let TerminalEvent::Settlement { generation, .. } = event else {
        return Ok(false);
    };
    if binding.attaching.is_none() && binding.held.is_none() {
        return Ok(false);
    }
    let target = match &binding.held {
        Some(TerminalAttachment::Owned(value)) => Some(value.generation),
        Some(TerminalAttachment::Blocked { generation, .. }) => Some(*generation),
        None => None,
    };
    if target.is_some_and(|target| target != *generation) {
        return Ok(false);
    }
    if let Some(previous) = binding.pending_settlement.iter_mut().find(|previous| {
        matches!(previous, TerminalEvent::Settlement { generation: previous, .. } if previous == generation)
    }) { *previous = event.clone(); }
    else {
        if binding.pending_settlement.len() >= 4 { return Err(PROTOCOL); }
        binding.pending_settlement.push_back(event.clone());
    }
    Ok(true)
}
pub(crate) fn apply_settlement(binding: &mut Binding, connection: u64) -> Result<()> {
    let cursor = binding.cursor.as_mut().ok_or(STALE)?;
    for event in binding.pending_settlement.drain(..) {
        cursor.observe(&event, connection).map_err(|_| STALE)?;
    }
    Ok(())
}
