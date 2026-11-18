//! Existing native registry and engine queue, one view per tab, one shared client.
use super::*;
use crate::client::{runtime::*, terminal, transport::NativeTransport};
pub(in crate::client) type NativeView = Arc<tokio::sync::Mutex<ManagerRecovery<NativeTransport>>>;
pub(in crate::client) struct NativeBinding {
    pub terminal: u64,
    pub view: NativeView,
    pub state: Arc<Mutex<ViewState>>,
    pub shared: Arc<crate::engine::Shared>,
    pub(super) input: Mutex<super::input::Lane>,
    pub settled_at: Mutex<Option<Option<String>>>,
    pub attaching: Mutex<Option<Operation>>,
    pub acking: Mutex<Option<Operation>>,
}
impl NativeBinding {
    pub fn retire(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.retired = true;
            state.generation = state.generation.saturating_add(1);
            state.origin = None;
            state.installed = None;
            state.pending_ack = None;
            state.data.clear();
        }
        if let Ok(mut queue) = self.shared.queue.lock() {
            queue.discard_pending();
        }
        let _ = super::input::fence(self);
        for slot in [&self.attaching, &self.acking] {
            if let Ok(mut op) = slot.lock() {
                if let Some(op) = op.take() {
                    op.cancel.cancel();
                }
            }
        }
    }
    pub fn origin(&self) -> Result<RecoveryOrigin> {
        let origin = self
            .state
            .lock()
            .map_err(|_| PANIC)?
            .origin
            .clone()
            .ok_or(STALE)?;
        origin
            .witness
            .commit_if_current(|| {
                let state = self.state.lock().map_err(|_| PANIC)?;
                origin.check(&state).map_err(|_| STALE)?;
                if state
                    .cursor
                    .as_ref()
                    .is_none_or(|c| c.binding() != origin.witness.binding())
                {
                    return Err(STALE);
                }
                Ok(())
            })
            .map_err(super::operations::client_error)??;
        Ok(origin)
    }
    pub fn ready(&self) -> Result<bool> {
        if self.shared.owner != std::thread::current().id() {
            return Err(crate::Status::WrongThread as i32);
        }
        let state = self.state.lock().map_err(|_| PANIC)?;
        let queue = self.shared.queue.lock().map_err(|_| PANIC)?;
        Ok(!state.retired
            && !queue.closed
            && !queue.failed
            && state.cursor.is_some()
            && state.origin.is_some()
            && state
                .installed
                .is_none_or(|(ticket, _)| queue.consumed >= ticket))
    }
}
impl Drop for NativeBinding {
    fn drop(&mut self) {
        self.retire();
    }
}
pub(in crate::client) fn bind(mobile: &Mobile, tab: m::Id, engine: u64) -> Result<()> {
    let client = mobile.manager()?.clone();
    let shared = crate::engine::lookup(engine).map_err(|e| e as i32)?;
    let receiver = VerifiedNativeExtension::verify().map_err(|_| PROTOCOL)?;
    let mut owners = terminal::owners().lock().map_err(|_| PANIC)?;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed {
        return Err(STALE);
    }
    if owners.contains_key(&engine) || s.managers.contains_key(&tab) || s.managers.len() >= 32 {
        return Err(BUSY);
    }
    let view = ManagerRecovery::bind_native0(client, engine, receiver, SnapshotLimits::default())
        .map_err(super::operations::recovery_error)?;
    let state = view.state.clone();
    let settled = s.manager_catalog.as_ref().and_then(|catalog| {
        catalog.entries.iter().find_map(|entry| match entry {
            m::CatalogEntry::Session { value } if value.tabs.iter().any(|t| t.id == tab) => {
                value.facts.settled_at.known().cloned()
            }
            _ => None,
        })
    });
    let binding = Arc::new(NativeBinding {
        terminal: engine,
        view: Arc::new(tokio::sync::Mutex::new(view)),
        state,
        shared,
        input: Mutex::new(super::input::Lane::default()),
        settled_at: Mutex::new(settled),
        attaching: Mutex::new(None),
        acking: Mutex::new(None),
    });
    owners.insert(engine, mobile.id);
    s.managers.insert(tab, binding);
    Ok(())
}
pub(in crate::client) fn by_engine(mobile: &Mobile, engine: u64) -> Result<Arc<NativeBinding>> {
    mobile.manager()?;
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed {
        return Err(STALE);
    }
    s.managers
        .values()
        .find(|b| b.terminal == engine)
        .cloned()
        .ok_or(STALE)
}
pub(in crate::client) fn by_tab(mobile: &Mobile, tab: m::Id) -> Result<Arc<NativeBinding>> {
    mobile.manager()?;
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed {
        return Err(STALE);
    }
    s.managers.get(&tab).cloned().ok_or(STALE)
}
pub(in crate::client) fn unbind(mobile: &Mobile, engine: u64) -> Result<()> {
    mobile.manager()?;
    let mut owners = terminal::owners().lock().map_err(|_| PANIC)?;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    let tab = s
        .managers
        .iter()
        .find(|(_, b)| b.terminal == engine)
        .map(|(id, _)| *id)
        .ok_or(STALE)?;
    let binding = s.managers.remove(&tab).ok_or(STALE)?;
    owners.remove(&engine);
    drop(s);
    drop(owners);
    binding.retire();
    Ok(())
}
pub(in crate::client) fn fence(mobile: &Mobile) -> Result<()> {
    let views = {
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        if s.closed {
            return Err(STALE);
        }
        s.generation = s.generation.checked_add(1).ok_or(BUSY)?;
        for operation in s.operations.values() {
            operation.cancel.cancel();
        }
        let pending: Vec<_> = s.requests.drain().collect();
        for (id, p) in pending {
            if p.dispatched {
                s.cancels.push_back((p.generation, id));
            }
        }
        s.outbound.clear();
        s.managers.values().cloned().collect::<Vec<_>>()
    };
    for binding in views {
        binding.retire();
    }
    Ok(())
}
pub(in crate::client) fn retire_all(mobile: &Mobile) -> Result<()> {
    let views = {
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        s.manager_catalog = None;
        s.managers.drain().map(|(_, b)| b).collect::<Vec<_>>()
    };
    for b in views {
        b.retire();
    }
    Ok(())
}
pub(in crate::client) fn consumed(mobile: Arc<Mobile>, engine: u64) -> Result<()> {
    let b = by_engine(&mobile, engine)?;
    if !b.ready()? {
        return Err(BUSY);
    }
    let mut slot = b.acking.lock().map_err(|_| PANIC)?;
    if slot.is_some() {
        return Err(BUSY);
    }
    {
        let state = b.state.lock().map_err(|_| PANIC)?;
        let queue = b.shared.queue.lock().map_err(|_| PANIC)?;
        if state.pending_ack.is_none()
            && state.installed.is_none()
            && !state
                .data
                .iter()
                .any(|(ticket, _)| *ticket <= queue.consumed)
        {
            return Ok(());
        }
    }
    let operation = {
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        reserve_locked(&mut s, CompletionOrigin::NativeInternal, SMALL_RESERVE)?
    };
    *slot = Some(operation.clone());
    drop(slot);
    let exec = runtime()?;
    start(exec, mobile, operation, async move {
        let view = b.view.lock().await;
        let result = view
            .acknowledge_consumed()
            .await
            .map_err(super::operations::recovery_error)?;
        serde_json::to_value(result).map_err(|_| PROTOCOL)
    });
    Ok(())
}
