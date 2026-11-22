use super::{terminal::Binding, transport::Pending};
#[path = "client_events.rs"]
mod events;
use super::profile::Driver;
use events::finish;
use shellfox_client::{Client, ClientError, ConnectionStatus};
use shellfox_protocol::{CancellationToken, ClientId, Permission, TabId};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock},
};
use tokio::runtime::{Builder, Runtime};

pub(super) const BUSY: i32 = 1;
pub(super) const INVALID: i32 = 2;
pub(super) const STALE: i32 = 3;
pub(super) const PANIC: i32 = 6;
pub(super) const EMPTY: i32 = 7;
pub(super) const BUFFER: i32 = 8;
pub(super) const CANCELLED: i32 = 9;
pub(super) const NETWORK: i32 = 10;
pub(super) const PROTOCOL: i32 = 11;
pub(super) type Result<T> = std::result::Result<T, i32>;
pub(super) const RESULT_RESERVE: usize = shellfox_protocol::MAX_RESPONSE_BYTES + 65536;
pub(super) const SMALL_RESERVE: usize = 16 * 1024;
const EVENT_BUDGET: usize = 32 * 1024 * 1024;

impl Mobile {
    /// Ask the native owner for a dispatch/read_event pass.
    pub fn wake(&self) {
        self.waker.wake(self.id);
    }
}

pub(super) struct Mobile {
    pub id: u64,
    pub driver: Driver,
    pub state: Mutex<State>,
    pub dispatch_gate: Mutex<()>,
    pub waker: super::waker::Waker,
    request_slots: tokio::sync::Semaphore,
    mutation_slots: tokio::sync::Semaphore,
}
pub(super) struct State {
    pub closed: bool,
    pub generation: u64,
    pub next_operation: u64,
    pub next_request: u64,
    pub next_reset_ticket: u64,
    pub operations: HashMap<u64, Operation>,
    pub reserved_bytes: usize,
    pub requests: HashMap<u64, Pending>,
    pub outbound: VecDeque<u64>,
    pub cancels: VecDeque<(u64, u64)>,
    pub events: VecDeque<Vec<u8>>,
    pub event_bytes: usize,
    pub bindings: HashMap<TabId, Binding>,
    pub observers: HashMap<shellfox_protocol::observer_v2::Id, super::observer::DeliveryBinding>,
    pub observer_withdrawing: bool,
    pub observer_disconnect: Option<u64>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            closed: false,
            generation: 1,
            next_operation: 1,
            next_request: 1,
            next_reset_ticket: 1,
            operations: HashMap::new(),
            reserved_bytes: 0,
            requests: HashMap::new(),
            outbound: VecDeque::new(),
            cancels: VecDeque::new(),
            events: VecDeque::new(),
            event_bytes: 0,
            bindings: HashMap::new(),
            observers: HashMap::new(),
            observer_withdrawing: false,
            observer_disconnect: None,
        }
    }
}
struct Registry {
    next: u64,
    clients: HashMap<u64, Arc<Mobile>>,
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
pub(super) fn registry_lookup(id: u64) -> Result<Arc<Mobile>> {
    registry()
        .lock()
        .map_err(|_| PANIC)?
        .clients
        .get(&id)
        .cloned()
        .ok_or(STALE)
}
fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| {
        Mutex::new(Registry {
            next: 1,
            clients: HashMap::new(),
        })
    })
}
pub(super) fn remove(id: u64) -> Result<Arc<Mobile>> {
    registry()
        .lock()
        .map_err(|_| PANIC)?
        .clients
        .remove(&id)
        .ok_or(STALE)
}
#[path = "client_creation.rs"]
mod creation;
pub(super) use creation::{create, create_observer};
pub(super) fn runtime() -> Result<&'static Runtime> {
    static RUNTIME: OnceLock<std::result::Result<Runtime, std::io::Error>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("shellfox-client")
                .enable_time()
                .build()
        })
        .as_ref()
        .map_err(|_| PANIC)
}
pub(super) async fn request_slot(mobile: &Mobile) -> Result<tokio::sync::SemaphorePermit<'_>> {
    acquire(&mobile.request_slots).await
}
pub(super) async fn mutation_slot(mobile: &Mobile) -> Result<tokio::sync::SemaphorePermit<'_>> {
    acquire(&mobile.mutation_slots).await
}
async fn acquire(slots: &tokio::sync::Semaphore) -> Result<tokio::sync::SemaphorePermit<'_>> {
    let operation = CURRENT.try_with(Clone::clone).map_err(|_| STALE)?;
    tokio::select! { biased;
        _ = operation.cancel.cancelled() => Err(CANCELLED),
        permit = slots.acquire() => permit.map_err(|_| STALE),
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CompletionOrigin {
    Explicit,
    NativeInternal,
}
#[derive(Clone)]
pub(super) struct Operation {
    pub id: u64,
    pub generation: u64,
    pub cancel: CancellationToken,
    pub origin: CompletionOrigin,
    pub reservation: usize,
}
tokio::task_local! { pub(super) static CURRENT: Operation; }
pub(super) fn reserve_locked(
    s: &mut State,
    origin: CompletionOrigin,
    reservation: usize,
) -> Result<Operation> {
    if s.closed {
        return Err(STALE);
    }
    if s.operations.len() >= 128
        || s.events.len() + s.operations.len() >= 256
        || s.event_bytes + s.reserved_bytes + reservation > EVENT_BUDGET
    {
        return Err(BUSY);
    }
    let id = s.next_operation;
    s.next_operation = id.checked_add(1).ok_or(BUSY)?;
    let cancel = CancellationToken::new();
    let operation = Operation {
        id,
        generation: s.generation,
        cancel,
        origin,
        reservation,
    };
    s.reserved_bytes += reservation;
    s.operations.insert(id, operation.clone());
    Ok(operation)
}
pub(super) fn launch<F, Fut>(mobile: Arc<Mobile>, origin: CompletionOrigin, work: F) -> Result<u64>
where
    F: FnOnce(Arc<Mobile>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<serde_json::Value>> + Send + 'static,
{
    launch_reserved(mobile, origin, RESULT_RESERVE, work)
}
pub(super) fn launch_small<F, Fut>(
    mobile: Arc<Mobile>,
    origin: CompletionOrigin,
    work: F,
) -> Result<u64>
where
    F: FnOnce(Arc<Mobile>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<serde_json::Value>> + Send + 'static,
{
    launch_reserved(mobile, origin, SMALL_RESERVE, work)
}
fn launch_reserved<F, Fut>(
    mobile: Arc<Mobile>,
    origin: CompletionOrigin,
    reservation: usize,
    work: F,
) -> Result<u64>
where
    F: FnOnce(Arc<Mobile>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<serde_json::Value>> + Send + 'static,
{
    let executor = runtime()?;
    let operation = {
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        reserve_locked(&mut s, origin, reservation)?
    };
    let id = operation.id;
    let worker_mobile = mobile.clone();
    start(executor, mobile, operation, async move {
        work(worker_mobile).await
    });
    Ok(id)
}
pub(super) fn start<F>(
    executor: &'static Runtime,
    mobile: Arc<Mobile>,
    operation: Operation,
    work: F,
) where
    F: std::future::Future<Output = Result<serde_json::Value>> + Send + 'static,
{
    let task = executor.spawn(CURRENT.scope(operation.clone(), work));
    executor.spawn(async move {
        // Do not abort client futures; joining turns a Rust task panic into an event.
        let result = task.await.unwrap_or(Err(PANIC));
        finish(&mobile, &operation, result);
    });
}
pub(super) fn watch(
    executor: &'static Runtime,
    mobile: Arc<Mobile>,
    operation: Operation,
    receive: tokio::sync::oneshot::Receiver<Result<serde_json::Value>>,
) {
    executor.spawn(async move {
        finish(&mobile, &operation, receive.await.unwrap_or(Err(STALE)));
    });
}
pub(super) fn unreserve(s: &mut State, operation: &Operation) {
    if let Some(reserved) = s.operations.remove(&operation.id) {
        s.reserved_bytes -= reserved.reservation;
    }
}
pub(super) fn client_error(error: ClientError) -> i32 {
    match error {
        ClientError::Validation(_) => PROTOCOL,
        ClientError::Transport(shellfox_protocol::TransportError::Cancelled) => CANCELLED,
        ClientError::Transport(_) => NETWORK,
        ClientError::NotConnected | ClientError::StaleConnection => STALE,
        ClientError::TooManyRequests => BUSY,
    }
}
