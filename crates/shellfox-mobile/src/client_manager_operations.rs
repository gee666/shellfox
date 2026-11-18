use super::bindings::{self, NativeBinding};
use super::*;
use crate::client::runtime::*;
use shellfox_protocol::Validate;
pub(in crate::client) fn client_error(error: shellfox_client::manager_v2::ClientError) -> i32 {
    use shellfox_client::manager_v2::ClientError as E;
    match error {
        E::InvalidRequest | E::ProfileDenied => INVALID,
        E::InvalidResponse | E::Host(_) => PROTOCOL,
        E::Transport(shellfox_protocol::TransportError::Cancelled) => CANCELLED,
        E::Transport(_) => NETWORK,
        E::Busy => BUSY,
        E::NotConnected | E::NotDiscovered | E::StaleConnection => STALE,
    }
}
pub(in crate::client) fn recovery_error(error: ManagerRecoveryError) -> i32 {
    match error {
        ManagerRecoveryError::Client(e) => client_error(e),
        ManagerRecoveryError::Cursor(_) => PROTOCOL,
        ManagerRecoveryError::Engine(e) => e as i32,
    }
}
pub(in crate::client) fn cursor_error(error: CursorError) -> i32 {
    match error {
        CursorError::Busy
        | CursorError::Delivery(
            shellfox_client::observer_v2::CursorError::RecoveryNotConsumed
            | shellfox_client::observer_v2::CursorError::RecoveryRequired,
        ) => BUSY,
        _ => STALE,
    }
}
fn json<T: serde::Serialize>(value: T) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(|_| PROTOCOL)
}
fn metadata(value: m::Reply<m::Attachment>) -> Result<serde_json::Value> {
    let mut value = json(value)?;
    if let Some(recovery) = value
        .get_mut("value")
        .and_then(|v| v.get_mut("recovery"))
        .and_then(serde_json::Value::as_object_mut)
    {
        recovery.remove("data");
    }
    Ok(value)
}
fn reserve(mobile: &Mobile, b: &NativeBinding) -> Result<Operation> {
    let mut pending = b.attaching.lock().map_err(|_| PANIC)?;
    if pending.is_some() {
        return Err(BUSY);
    }
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    let op = reserve_locked(&mut s, CompletionOrigin::Explicit, RESULT_RESERVE)?;
    *pending = Some(op.clone());
    Ok(op)
}
pub(in crate::client) fn command(mobile: Arc<Mobile>, command: m::Command) -> Result<u64> {
    mobile.manager()?;
    command.validate().map_err(|_| INVALID)?;
    match command {
        m::Command::AcknowledgeTerminal(_)
        | m::Command::WriteTerminal(_)
        | m::Command::ResizeTerminal(_) => Err(INVALID),
        m::Command::GetSnapshot(request) => launch(
            mobile,
            CompletionOrigin::Explicit,
            move |mobile| async move {
                let witness = mobile.manager()?.binding_witness().map_err(client_error)?;
                let result = mobile
                    .manager()?
                    .execute_with_witness(&witness, &request)
                    .await
                    .map_err(client_error)?;
                if let m::Reply::Success(snapshot) = &result {
                    let catalog = Arc::new(snapshot.clone());
                    let operation = CURRENT.try_with(Clone::clone).map_err(|_| STALE)?;
                    witness
                        .commit_if_current(|| {
                            let mut s = mobile.state.lock().map_err(|_| PANIC)?;
                            if s.closed
                                || s.generation != operation.generation
                                || operation.cancel.is_cancelled()
                            {
                                return Err(STALE);
                            }
                            for (tab, b) in &s.managers {
                                let value = catalog.entries.iter().find_map(|entry| match entry {
                                    m::CatalogEntry::Session { value }
                                        if value.tabs.iter().any(|t| t.id == *tab) =>
                                    {
                                        value.facts.settled_at.known().cloned()
                                    }
                                    _ => None,
                                });
                                *b.settled_at.lock().map_err(|_| PANIC)? = value;
                            }
                            s.manager_catalog = Some(catalog);
                            Ok(())
                        })
                        .map_err(client_error)??;
                }
                json(result)
            },
        ),
        m::Command::AttachTerminal(request) => {
            let b = bindings::by_tab(&mobile, request.tab_id)?;
            let op = reserve(&mobile, &b)?;
            let id = op.id;
            super::input::fence(&b)?;
            start(runtime()?, mobile, op, async move {
                let mut view = b.view.lock().await;
                metadata(view.attach(request).await.map_err(recovery_error)?)
            });
            Ok(id)
        }
        m::Command::TakeTerminalControl(request) => {
            let b = bindings::by_tab(&mobile, request.delivery.tab_id)?;
            let origin = b.origin()?;
            origin
                .witness
                .commit_if_current(|| {
                    let state = b.state.lock().map_err(|_| PANIC)?;
                    origin.check(&state).map_err(|_| STALE)?;
                    if state
                        .cursor
                        .as_ref()
                        .is_none_or(|c| c.delivery() != &request.delivery)
                    {
                        return Err(STALE);
                    }
                    Ok(())
                })
                .map_err(client_error)??;
            let op = reserve(&mobile, &b)?;
            let id = op.id;
            start(runtime()?, mobile, op, async move {
                let mut view = b.view.lock().await;
                metadata(view.take_control().await.map_err(recovery_error)?)
            });
            Ok(id)
        }
        m::Command::DetachTerminal(request) => {
            let b = bindings::by_tab(&mobile, request.delivery.tab_id)?;
            let origin = b.origin()?;
            let actual = origin
                .witness
                .commit_if_current(|| {
                    let mut state = b.state.lock().map_err(|_| PANIC)?;
                    origin.check(&state).map_err(|_| STALE)?;
                    let c = state.cursor.as_mut().ok_or(STALE)?;
                    c.validate_detach(&request, origin.witness.binding())
                        .map_err(|_| STALE)?;
                    let actual = c
                        .detach_request(request.release_control.is_some(), origin.witness.binding())
                        .map_err(|_| STALE)?;
                    if actual.release_control != request.release_control {
                        return Err(STALE);
                    }
                    Ok(actual)
                })
                .map_err(client_error)??;
            b.retire();
            launch(
                mobile,
                CompletionOrigin::Explicit,
                move |mobile| async move {
                    json(
                        mobile
                            .manager()?
                            .execute_with_witness(&origin.witness, &actual)
                            .await
                            .map_err(client_error)?,
                    )
                },
            )
        }
        command => launch(
            mobile,
            CompletionOrigin::Explicit,
            move |mobile| async move {
                let _slot = request_slot(&mobile).await?;
                json(
                    mobile
                        .manager()?
                        .execute_command(&command)
                        .await
                        .map_err(client_error)?,
                )
            },
        ),
    }
}
pub(in crate::client) fn poll(mobile: Arc<Mobile>) -> Result<u64> {
    mobile.manager()?;
    launch(
        mobile,
        CompletionOrigin::NativeInternal,
        move |mobile| async move {
            let witness = mobile.manager()?.binding_witness().map_err(client_error)?;
            let views = {
                let s = mobile.state.lock().map_err(|_| PANIC)?;
                s.managers
                    .iter()
                    .map(|(tab, b)| (*tab, b.clone()))
                    .collect::<Vec<_>>()
            };
            let mut targets = Vec::new();
            for (tab, b) in views {
                if let Ok(origin) = b.origin() {
                    if origin.witness.same_origin(&witness) {
                        targets.push(super::global_poll::Target {
                            origin,
                            state: b.state.clone(),
                            shared: b.shared.clone(),
                            tab,
                        });
                    }
                }
            }
            let context = super::global_poll::ManagerPollContext::captured(witness, targets)
                .map_err(recovery_error)?;
            json(
                context
                    .poll(mobile.manager()?.as_ref())
                    .await
                    .map_err(recovery_error)?,
            )
        },
    )
}
