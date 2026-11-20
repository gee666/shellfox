use super::*;
use files::{TransferReply, TransferRequest};
use shellfox_remote::transfer_bridge;
use std::{
    collections::{HashMap, VecDeque},
    time::Instant,
};
use uuid::Uuid;

#[derive(Clone, PartialEq, Eq)]
struct Owner {
    key: String,
    view: u64,
    approval: u64,
}
impl Owner {
    fn of(actor: &Actor) -> Self {
        Self {
            key: key(actor),
            view: actor.view_epoch(),
            approval: actor.approval_epoch,
        }
    }
}

struct Binding {
    owner: Owner,
    scope: Scope,
    touched: Instant,
    write: bool,
}

struct Active {
    binding: Binding,
    worker: Worker,
}

struct Receipt {
    id: Uuid,
    binding: Binding,
    result: files::WriteResult,
}

#[derive(Default)]
pub(in crate::remote) struct Transfers {
    active: HashMap<Uuid, Active>,
    receipts: VecDeque<Receipt>,
}

impl Transfers {
    pub(in crate::remote) fn cancel_actor(&mut self, actor: &Actor) {
        self.cancel_view(&key(actor), actor.view_epoch());
    }

    pub(in crate::remote) fn cancel_view(&mut self, key: &str, view: u64) {
        let retain = |stored: &Owner| stored.key != key || stored.view != view;
        self.active.retain(|_, entry| retain(&entry.binding.owner));
        self.receipts.retain(|entry| retain(&entry.binding.owner));
    }

    fn sweep(&mut self) {
        let ttl = files::TransferLimits::default().idle_ttl;
        self.active
            .retain(|_, entry| entry.binding.touched.elapsed() <= ttl);
        self.receipts
            .retain(|entry| entry.binding.touched.elapsed() <= ttl);
    }

    pub(super) fn begin(
        &mut self,
        actor: &Actor,
        scope: Scope,
        location: files::Location,
        command: &RemoteCommand,
    ) -> Result<serde_json::Value, AppError> {
        self.sweep();
        let owner = Owner::of(actor);
        if self.active.len() >= files::TRANSFERS_TOTAL
            || self
                .active
                .values()
                .filter(|entry| entry.binding.owner == owner)
                .count()
                >= files::TRANSFERS_PER_ACTOR
        {
            return Err(failure(
                ErrorCode::Validation,
                "WSL Files transfer limit reached.",
            ));
        }
        let mut request = transfer_bridge::decode(command)?;
        let write = match &mut request {
            TransferRequest::BeginRead(input) => {
                input.path = location.path;
                false
            }
            TransferRequest::BeginWrite(input) => {
                input.path = location.path;
                true
            }
            _ => return Err(invalid_reply()),
        };
        let worker = scope.start()?;
        let reply = worker.transfer(request).map_err(worker_error)?;
        let id = match &reply {
            TransferReply::ReadStarted(result)
                if !write
                    && result.path.starts_with('/')
                    && scope.location(&result.path)?.path == result.path =>
            {
                result.transfer_id
            }
            TransferReply::WriteStarted(result) if write => result.transfer_id,
            _ => return Err(invalid_reply()),
        };
        if self.active.contains_key(&id) || self.receipts.iter().any(|receipt| receipt.id == id) {
            return Err(invalid_reply());
        }
        let result = transfer_bridge::encode(reply)?;
        let binding = Binding {
            owner,
            scope,
            touched: Instant::now(),
            write,
        };
        self.active.insert(id, Active { binding, worker });
        Ok(result)
    }

    pub(super) fn continue_request(
        &mut self,
        service: &Service,
        actor: &Actor,
        command: &RemoteCommand,
    ) -> Result<Option<serde_json::Value>, AppError> {
        let request = transfer_bridge::decode(command)?;
        let id = request.id().ok_or_else(invalid_reply)?;
        self.sweep();
        if let Some(index) = self.receipts.iter().position(|entry| entry.id == id) {
            let binding = &self.receipts[index].binding;
            check_owner(binding, actor)?;
            if let Err(error) = check_scope(binding, service, actor) {
                self.receipts.remove(index);
                return Err(error);
            }
            return match request {
                TransferRequest::Finish(_) => transfer_bridge::encode(TransferReply::Finished(
                    self.receipts[index].result.clone(),
                ))
                .map(Some),
                TransferRequest::Cancel(_) => {
                    self.receipts.remove(index);
                    transfer_bridge::encode(TransferReply::Cancelled).map(Some)
                }
                _ => Err(not_found()),
            };
        }
        let Some(entry) = self.active.get(&id) else {
            return Ok(None);
        };
        // A guessed handle cannot cause another actor's worker to be dropped.
        check_owner(&entry.binding, actor)?;
        let mut entry = self.active.remove(&id).ok_or_else(not_found)?;
        check_scope(&entry.binding, service, actor)?;
        let finish = matches!(request, TransferRequest::Finish(_));
        let cancel = matches!(request, TransferRequest::Cancel(_));
        let reply = match entry.worker.transfer(request) {
            Ok(reply) => reply,
            Err(error) => {
                if !finish
                    && !cancel
                    && error
                        .downcast_ref::<files::FileError>()
                        .is_some_and(|error| error.code == files::ErrorCode::Validation)
                {
                    self.active.insert(id, entry);
                }
                return Err(worker_error(error));
            }
        };
        let valid = match &reply {
            TransferReply::Chunk(_) => !entry.binding.write && !finish && !cancel,
            TransferReply::Progress(_) => entry.binding.write && !finish && !cancel,
            TransferReply::Finished(result) => {
                entry.binding.write
                    && finish
                    && result.path.starts_with('/')
                    && entry.binding.scope.location(&result.path)?.path == result.path
            }
            TransferReply::Cancelled => cancel,
            _ => false,
        };
        if !valid {
            return Err(invalid_reply());
        }
        entry.binding.touched = Instant::now();
        if let TransferReply::Finished(result) = &reply {
            if self.receipts.len() == files::TRANSFER_RECEIPTS {
                self.receipts.pop_front();
            }
            self.receipts.push_back(Receipt {
                id,
                binding: entry.binding,
                result: result.clone(),
            });
        } else if !cancel {
            self.active.insert(id, entry);
        }
        transfer_bridge::encode(reply).map(Some)
    }
}

fn check_owner(binding: &Binding, actor: &Actor) -> Result<(), AppError> {
    if binding.owner != Owner::of(actor) {
        return Err(not_found());
    }
    Ok(())
}

fn check_scope(binding: &Binding, service: &Service, actor: &Actor) -> Result<(), AppError> {
    let scope = &binding.scope;
    authorize(actor, scope.guest.guest_cwd(), binding.write)?;
    let original = Scope::resolve(service, Some(scope.session), Some(scope.tab))?;
    if original.is_none_or(|current| {
        current.account != scope.account
            || !current
                .guest
                .distro()
                .eq_ignore_ascii_case(scope.guest.distro())
    }) {
        return Err(failure(
            ErrorCode::TargetLost,
            "The original WSL Files tab changed.",
        ));
    }
    Ok(())
}

fn not_found() -> AppError {
    failure(
        ErrorCode::NotFound,
        "Transfer not found for this connection generation.",
    )
}
