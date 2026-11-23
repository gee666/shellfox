use super::*;


pub(crate) fn reset_ticket(mobile: &Mobile, terminal: u64) -> Result<u64> {
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    let b = s
        .bindings
        .values()
        .find(|b| b.terminal == terminal)
        .ok_or(STALE)?;
    if s.closed {
        return Err(STALE);
    }
    if !b.reset_required {
        return Err(EMPTY);
    }
    Ok(b.reset_ticket)
}
pub(crate) fn reset_grid(mobile: &Mobile, terminal: u64, ticket: u64) -> Result<(u16, u16)> {
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    let b = s
        .bindings
        .values()
        .find(|b| b.terminal == terminal)
        .ok_or(STALE)?;
    if s.closed || !b.reset_required || b.reset_ticket != ticket {
        return Err(STALE);
    }
    b.required_grid.ok_or(EMPTY)
}
pub(crate) fn reset_done(mobile: &Mobile, terminal: u64, ticket: u64) -> Result<()> {
    let connection = mobile.client.state().connection_id;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    let b = s
        .bindings
        .values_mut()
        .find(|b| b.terminal == terminal)
        .ok_or(STALE)?;
    if !b.reset_required || b.reset_ticket != ticket {
        return Err(STALE);
    }
    let shared = crate::engine::lookup(terminal).map_err(|e| e as i32)?;
    if shared.owner != std::thread::current().id() {
        return Err(crate::Status::WrongThread as i32);
    }
    let q = shared.queue.lock().map_err(|_| PANIC)?;
    if q.failed || q.closed || q.accepted != 0 || q.consumed != 0 || !q.chunks.is_empty() {
        return Err(BUSY);
    }
    drop(q);
    if let Some((cols, rows)) = b.required_grid {
        let grid = crate::engine::with_engine(terminal, |owned| {
            let screen = owned.terminal.render().map_err(|_| crate::Status::Engine)?;
            Ok((screen.cols, screen.rows))
        })
        .map_err(|status| status as i32)?;
        if grid != (cols, rows) {
            return Err(INVALID);
        }
    }
    if let Some(value) = b.held.clone() {
        install(b, &value, connection)?;
        b.held = None;
    }
    b.reset_required = false;
    Ok(())
}

pub(crate) fn ready(binding: &Binding) -> Result<bool> {
    if binding.reset_required || binding.cursor.is_none() || binding.attaching.is_some() {
        return Ok(false);
    }
    if !binding.recovering {
        return Ok(true);
    }
    let Some(ticket) = binding.recovery_ticket else {
        return Ok(false);
    };
    let shared = crate::engine::lookup(binding.terminal).map_err(|e| e as i32)?;
    let queue = shared.queue.lock().map_err(|_| PANIC)?;
    Ok(!queue.failed && !queue.closed && queue.consumed >= ticket)
}
pub(crate) fn ready_handle(mobile: &Mobile, terminal: u64) -> Result<bool> {
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed {
        return Err(STALE);
    }
    let b = s
        .bindings
        .values()
        .find(|b| b.terminal == terminal)
        .ok_or(STALE)?;
    ready(b)
}
