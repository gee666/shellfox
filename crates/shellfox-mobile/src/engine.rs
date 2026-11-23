use crate::{queue::Queue, MAX_PUMP};
use shellfox_ghostty_vt::Terminal;
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    thread::{self, ThreadId},
};

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok = 0,
    Busy = 1,
    Invalid = 2,
    Stale = 3,
    WrongThread = 4,
    Engine = 5,
    Panic = 6,
}
pub(crate) type Result<T> = std::result::Result<T, Status>;
pub(crate) struct Shared {
    pub owner: ThreadId,
    pub queue: Mutex<Queue>,
    // Sticky Manager association and passive-parser role, until engine close.
}
pub(crate) struct Owned {
    pub terminal: Terminal,
    pub composer: crate::composer::Composer,
    pub colors: Option<shellfox_ghostty_vt::TerminalColors>,
    pub shared: Arc<Shared>,
}
struct Registry {
    next: u64,
    handles: HashMap<u64, Arc<Shared>>,
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| {
        Mutex::new(Registry {
            next: 1,
            handles: HashMap::new(),
        })
    })
}
#[derive(Default)]
struct Engines(HashMap<u64, Owned>);
impl Drop for Engines {
    fn drop(&mut self) {
        if let Ok(mut registry) = registry().lock() {
            for (id, owned) in &self.0 {
                if let Ok(mut q) = owned.shared.queue.lock() {
                    q.closed = true;
                    q.fail();
                }
                registry.handles.remove(id);
            }
        }
    }
}
thread_local! { static ENGINES: RefCell<Engines> = RefCell::new(Engines::default()); }

pub(crate) fn dimensions(cols: u16, rows: u16) -> Result<()> {
    if cols == 0
        || rows == 0
        || cols > 512
        || rows > 512
        || u32::from(cols) * u32::from(rows) > 65536
    {
        return Err(Status::Invalid);
    }
    Ok(())
}
pub(crate) fn lookup(id: u64) -> Result<Arc<Shared>> {
    registry()
        .lock()
        .map_err(|_| Status::Panic)?
        .handles
        .get(&id)
        .cloned()
        .ok_or(Status::Stale)
}
pub(crate) fn create(cols: u16, rows: u16) -> Result<u64> {
    dimensions(cols, rows)?;
    let mut registry = registry().lock().map_err(|_| Status::Panic)?;
    if registry.handles.len() >= 32 {
        return Err(Status::Busy);
    }
    let id = registry.next;
    registry.next = id.checked_add(1).ok_or(Status::Busy)?;
    let terminal = Terminal::new(cols, rows).map_err(|_| Status::Engine)?;
    let shared = Arc::new(Shared {
        owner: thread::current().id(),
        queue: Mutex::new(Queue::default()),
    });
    ENGINES.with(|engines| {
        engines
            .try_borrow_mut()
            .map_err(|_| Status::Busy)?
            .0
            .insert(
                id,
                Owned {
                    terminal,
                    composer: Default::default(),
                    colors: None,
                    shared: shared.clone(),
                },
            );
        Ok(())
    })?;
    registry.handles.insert(id, shared);
    Ok(id)
}
pub(crate) fn with_engine<T>(id: u64, action: impl FnOnce(&mut Owned) -> Result<T>) -> Result<T> {
    let shared = lookup(id)?;
    if shared.owner != thread::current().id() {
        return Err(Status::WrongThread);
    }
    ENGINES.with(|engines| {
        let mut engines = engines.try_borrow_mut().map_err(|_| Status::Busy)?;
        let owned = engines.0.get_mut(&id).ok_or(Status::Stale)?;
        if owned.shared.queue.lock().map_err(|_| Status::Panic)?.failed {
            return Err(Status::Engine);
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(owned)))
            .unwrap_or(Err(Status::Panic));
        if matches!(result, Err(Status::Engine | Status::Panic)) {
            owned.shared.queue.lock().map_err(|_| Status::Panic)?.fail();
        }
        result
    })
}
pub(crate) fn reset(id: u64, cols: u16, rows: u16) -> Result<()> {
    dimensions(cols, rows)?;
    let shared = lookup(id)?;
    if shared.owner != thread::current().id() {
        return Err(Status::WrongThread);
    }
    ENGINES.with(|engines| {
        let mut engines = engines.try_borrow_mut().map_err(|_| Status::Busy)?;
        let owned = engines.0.get_mut(&id).ok_or(Status::Stale)?;
        let mut q = shared.queue.lock().map_err(|_| Status::Panic)?;
        let mut terminal = Terminal::new(cols, rows).map_err(|_| Status::Engine)?;
        if let Some(colors) = &owned.colors {
            terminal.set_colors(colors).map_err(|_| Status::Engine)?;
        }
        owned.terminal = terminal;
        // The keyboard editor described the old screen.
        owned.composer.reset();
        *q = Queue::default();
        Ok(())
    })
}
pub(crate) fn close(id: u64) -> Result<()> {
    let shared = lookup(id)?;
    if shared.owner != thread::current().id() {
        return Err(Status::WrongThread);
    }
    ENGINES.with(|engines| {
        engines
            .try_borrow_mut()
            .map_err(|_| Status::Busy)?
            .0
            .remove(&id)
            .ok_or(Status::Stale)?;
        Ok(())
    })?;
    let mut registry = registry().lock().map_err(|_| Status::Panic)?;
    // Destruction may recover a poisoned queue only to discard it. Never leave
    // a registered handle after its thread-local engine has been removed.
    let mut q = shared
        .queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    q.closed = true;
    q.fail();
    registry.handles.remove(&id);
    Ok(())
}
pub(crate) fn pump(owned: &mut Owned) -> Result<()> {
    let mut budget = MAX_PUMP;
    loop {
        let next = {
            let mut q = owned.shared.queue.lock().map_err(|_| Status::Panic)?;
            match q.chunks.front() {
                Some((_, bytes)) if bytes.len() <= budget => {
                    let next = q.chunks.pop_front().ok_or(Status::Panic)?;
                    q.bytes -= next.1.len();
                    Some(next)
                }
                _ => None,
            }
        };
        let Some((sequence, payload)) = next else {
            return Ok(());
        };
        let crate::queue::Payload::Bytes(bytes) = payload;
        owned.terminal.feed(&bytes).map_err(|_| Status::Engine)?;
        // This view mirrors the host, which already answers terminal queries.
        owned.terminal.drain_replies().map_err(|_| Status::Engine)?;
        budget -= bytes.len();
        owned
            .shared
            .queue
            .lock()
            .map_err(|_| Status::Panic)?
            .consumed = sequence;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_panic_poisoning_requires_reset() {
        let id = create(20, 5).unwrap();
        assert_eq!(
            with_engine::<()>(id, |_| panic!("fixture panic")),
            Err(Status::Panic)
        );
        assert_eq!(with_engine(id, |_| Ok(())), Err(Status::Engine));
        reset(id, 20, 5).unwrap();
        assert_eq!(with_engine(id, |_| Ok(())), Ok(()));
        close(id).unwrap();
    }

    #[test]
    fn close_discards_a_poisoned_queue_without_leaking_handle() {
        let id = create(20, 5).unwrap();
        let shared = lookup(id).unwrap();
        let _ = std::panic::catch_unwind(|| {
            let _guard = shared.queue.lock().unwrap();
            panic!("fixture poisoned mutex");
        });
        close(id).unwrap();
        assert!(matches!(lookup(id), Err(Status::Stale)));
    }
}
