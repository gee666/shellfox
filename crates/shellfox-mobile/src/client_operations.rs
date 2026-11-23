use super::{runtime::*, terminal};
use shellfox_protocol::*;
use std::sync::Arc;

fn json<T: serde::Serialize>(value: T) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(|_| PROTOCOL)
}
pub(super) fn validate(mobile: &Mobile, command: &RemoteCommand) -> Result<()> {
    command.validate().map_err(|_| INVALID)?;
    // Only native consumption can grant credit. Caller-supplied host ACKs are unsafe.
    match command {
        RemoteCommand::AcknowledgeTerminal(_) => Err(INVALID),
        RemoteCommand::AttachTerminal(v) => {
            let s = mobile.state.lock().map_err(|_| PANIC)?;
            let binding = s.bindings.get(&v.tab_id).ok_or(INVALID)?;
            if binding.reset_required || binding.attaching.is_some() {
                Err(BUSY)
            } else {
                Ok(())
            }
        }
        RemoteCommand::WriteTerminal(v) => {
            let connection = mobile.client.state().connection_id;
            let s = mobile.state.lock().map_err(|_| PANIC)?;
            super::mutations::validate(s.bindings.get(&v.tab_id).ok_or(STALE)?, command, connection)
        }
        RemoteCommand::ResizeTerminal(v) => {
            let connection = mobile.client.state().connection_id;
            let s = mobile.state.lock().map_err(|_| PANIC)?;
            super::mutations::validate(s.bindings.get(&v.tab_id).ok_or(STALE)?, command, connection)
        }
        _ => Ok(()),
    }
}
pub(super) fn accept(mobile: Arc<Mobile>, value: RemoteCommand) -> Result<u64> {
    // The JS-facing command channel has no raw terminal-input or credit capability.
    // Native encoded input enters the FIFO through terminal_write[_ex] instead.
    if matches!(
        value,
        RemoteCommand::WriteTerminal(_) | RemoteCommand::AcknowledgeTerminal(_)
    ) {
        return Err(INVALID);
    }
    validate(&mobile, &value)?;
    match value {
        RemoteCommand::AttachTerminal(request) => accept_attachment(mobile, request),
        command @ RemoteCommand::ResizeTerminal(_) => {
            super::mutations::enqueue(mobile, command, CompletionOrigin::Explicit)
        }
        value @ (RemoteCommand::BeginTerminalUpload(_)
        | RemoteCommand::WriteTerminalUpload(_)
        | RemoteCommand::FinishTerminalUpload(_)
        | RemoteCommand::CancelTerminalUpload(_)) => super::uploads::accept(mobile, value),
        value => launch(mobile, CompletionOrigin::Explicit, |mobile| {
            command(mobile, value)
        }),
    }
}
fn accept_attachment(mobile: Arc<Mobile>, request: AttachTerminal) -> Result<u64> {
    let executor = runtime()?;
    let operation = {
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        let b = s.bindings.get(&request.tab_id).ok_or(INVALID)?;
        if b.reset_required || b.attaching.is_some() {
            return Err(BUSY);
        }
        let operation = reserve_locked(&mut s, CompletionOrigin::Explicit, RESULT_RESERVE)?;
        let b = s.bindings.get_mut(&request.tab_id).ok_or(STALE)?;
        b.attaching = Some((operation.id, request.clone()));
        b.pending_settlement.clear();
        b.clear_early();
        b.lane.fence();
        b.uploads.fence();
        operation
    };
    let id = operation.id;
    start(executor, mobile.clone(), operation, async move {
        command(mobile, RemoteCommand::AttachTerminal(request)).await
    });
    Ok(id)
}
pub(super) async fn command(
    mobile: Arc<Mobile>,
    command: RemoteCommand,
) -> Result<serde_json::Value> {
    let _slot = request_slot(&mobile).await?;
    if let RemoteCommand::AttachTerminal(request) = &command {
        let id = CURRENT.try_with(|op| op.id).map_err(|_| STALE)?;
        let s = mobile.state.lock().map_err(|_| PANIC)?;
        if s.bindings
            .get(&request.tab_id)
            .and_then(|b| b.attaching.as_ref())
            .map(|(id, _)| *id)
            != Some(id)
        {
            return Err(STALE);
        }
    } else {
        validate(&mobile, &command)?;
    }
    match command {
        RemoteCommand::AttachTerminal(value) => {
            match mobile.client.execute(&value).await.map_err(client_error)? {
                Reply::Success(value) => Ok(
                    serde_json::json!({"ok":true,"value":terminal::attachment(&mobile, &value)?}),
                ),
                Reply::Failure(error) => json(Reply::<TerminalAttachment>::Failure(error)),
            }
        }
        RemoteCommand::AcknowledgeTerminal(_) => Err(INVALID),
        RemoteCommand::DetachTerminal(v) => {
            let result = mobile.client.execute(&v).await.map_err(client_error)?;
            if matches!(result, Reply::Success(_)) {
                if let Ok(mut s) = mobile.state.lock() {
                    if let Some(b) = s.bindings.get_mut(&v.tab_id) {
                        b.lane.fence();
                        b.uploads.fence();
                        b.cursor = None;
                    }
                }
            }
            json(result)
        }
        command => json(
            mobile
                .client
                .execute_command(&command)
                .await
                .map_err(client_error)?,
        ),
    }
}
pub(super) fn pair(mobile: Arc<Mobile>) -> Result<u64> {
    launch(mobile, CompletionOrigin::Explicit, |mobile| async move {
        json(mobile.client.pair().await.map_err(client_error)?)
    })
}
pub(super) fn poll(mobile: Arc<Mobile>) -> Result<u64> {
    terminal::poll_ready(&mobile)?;
    launch(
        mobile,
        CompletionOrigin::NativeInternal,
        |mobile| async move {
            match mobile.client.poll().await.map_err(client_error)? {
                Reply::Success(value) => terminal::events(&mobile, value),
                Reply::Failure(error) => json(Reply::<PollResponse>::Failure(error)),
            }
        },
    )
}
