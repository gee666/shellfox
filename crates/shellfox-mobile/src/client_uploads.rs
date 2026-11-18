use super::runtime::*;
use shellfox_client::{UploadCursor, UploadError, ViewCursor};
use shellfox_protocol::*;
use std::{collections::HashMap, sync::Arc};
#[path = "client_upload_result.rs"]
mod response;

pub(super) struct UploadRecord {
    cursor: UploadCursor,
    begin: BeginTerminalUpload,
    connection: u64,
    active: Option<Operation>,
}
#[derive(Default)]
pub(super) struct UploadBook {
    serial: u64,
    records: HashMap<UploadId, UploadRecord>,
    begin_active: Option<Operation>,
}
impl UploadBook {
    pub fn fence(&mut self) {
        self.serial = self.serial.saturating_add(1);
        if let Some(active) = self.begin_active.take() {
            active.cancel.cancel();
        }
        for record in self.records.values_mut() {
            record.cursor.failed();
            if let Some(active) = record.active.take() {
                active.cancel.cancel();
            }
        }
    }
}
impl Drop for UploadBook {
    fn drop(&mut self) {
        self.fence();
    }
}
pub(super) struct UploadTask {
    operation: Operation,
    tab: TabId,
    connection: u64,
    serial: u64,
    command: RemoteCommand,
}
fn tab(command: &RemoteCommand) -> Result<TabId> {
    match command {
        RemoteCommand::BeginTerminalUpload(v) => Ok(v.tab_id),
        RemoteCommand::WriteTerminalUpload(v) => Ok(v.tab_id),
        RemoteCommand::FinishTerminalUpload(v) => Ok(v.tab_id),
        RemoteCommand::CancelTerminalUpload(v) => Ok(v.tab_id),
        _ => Err(INVALID),
    }
}
fn upload_error(error: UploadError) -> i32 {
    match error {
        UploadError::Busy => BUSY,
        UploadError::Invalid => INVALID,
        _ => STALE,
    }
}
fn identity(record: &UploadRecord, generation: Generation, epoch: u64) -> Result<()> {
    if record.begin.generation != generation || record.begin.control_epoch != epoch {
        Err(STALE)
    } else {
        Ok(())
    }
}
fn prepare(
    book: &mut UploadBook,
    view: Option<&ViewCursor>,
    connection: u64,
    command: RemoteCommand,
    operation: &Operation,
) -> Result<RemoteCommand> {
    if book.serial == u64::MAX {
        return Err(BUSY);
    }
    match command {
        RemoteCommand::BeginTerminalUpload(v) => {
            if book.begin_active.is_some() || book.records.len() >= 4 {
                return Err(BUSY);
            }
            let expected = view
                .ok_or(STALE)?
                .begin_upload(v.file_name, v.size, connection)
                .map_err(|_| STALE)?;
            if expected.generation != v.generation || expected.control_epoch != v.control_epoch {
                return Err(STALE);
            }
            book.begin_active = Some(operation.clone());
            Ok(RemoteCommand::BeginTerminalUpload(expected))
        }
        RemoteCommand::WriteTerminalUpload(v) => {
            let record = book.records.get_mut(&v.upload_id).ok_or(STALE)?;
            identity(record, v.generation, v.control_epoch)?;
            if record.active.is_some() {
                return Err(BUSY);
            }
            if record.connection != connection {
                return Err(STALE);
            }
            if record.cursor.offset() != v.offset {
                return Err(INVALID);
            }
            let bytes = decode_chunk(&v.data).map_err(|_| INVALID)?;
            let expected = record
                .cursor
                .write(&bytes, view.ok_or(STALE)?, connection)
                .map_err(upload_error)?;
            record.active = Some(operation.clone());
            Ok(RemoteCommand::WriteTerminalUpload(expected))
        }
        RemoteCommand::FinishTerminalUpload(v) => {
            let record = book.records.get_mut(&v.upload_id).ok_or(STALE)?;
            identity(record, v.generation, v.control_epoch)?;
            if record.active.is_some() {
                return Err(BUSY);
            }
            if record.connection != connection {
                return Err(STALE);
            }
            let expected = record
                .cursor
                .finish(view.ok_or(STALE)?, connection)
                .map_err(upload_error)?;
            record.active = Some(operation.clone());
            Ok(RemoteCommand::FinishTerminalUpload(expected))
        }
        RemoteCommand::CancelTerminalUpload(v) => {
            let record = book.records.get_mut(&v.upload_id).ok_or(STALE)?;
            identity(record, v.generation, v.control_epoch)?;
            if record.active.is_some() {
                return Err(BUSY);
            }
            let expected = record.cursor.cancel_request();
            record.active = Some(operation.clone());
            Ok(RemoteCommand::CancelTerminalUpload(expected))
        }
        _ => Err(INVALID),
    }
}
pub(super) fn accept(mobile: Arc<Mobile>, command: RemoteCommand) -> Result<u64> {
    let executor = runtime()?;
    command.validate().map_err(|_| INVALID)?;
    let tab = tab(&command)?;
    let connection = mobile.legacy()?.state();
    if connection.status != shellfox_client::ConnectionStatus::Connected {
        return Err(STALE);
    }
    let task = {
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        let operation = reserve_locked(&mut s, CompletionOrigin::Explicit, RESULT_RESERVE)?;
        let prepared = (|| {
            let b = s.bindings.get_mut(&tab).ok_or(STALE)?;
            if b.reset_required
                || (!matches!(command, RemoteCommand::CancelTerminalUpload(_))
                    && !b.lane.available())
            {
                return Err(STALE);
            }
            if !matches!(command, RemoteCommand::CancelTerminalUpload(_))
                && !super::terminal::ready(b)?
            {
                return Err(BUSY);
            }
            let command = prepare(
                &mut b.uploads,
                b.cursor.as_ref(),
                connection.connection_id,
                command,
                &operation,
            )?;
            Ok((command, b.uploads.serial))
        })();
        match prepared {
            Ok((command, serial)) => UploadTask {
                operation,
                tab,
                connection: connection.connection_id,
                serial,
                command,
            },
            Err(error) => {
                unreserve(&mut s, &operation);
                return Err(error);
            }
        }
    };
    let id = task.operation.id;
    let operation = task.operation.clone();
    let owner = mobile.clone();
    let worker = executor.spawn(CURRENT.scope(operation.clone(), async move {
        let result = match request_slot(&owner).await {
            Ok(_slot) => owner
                .legacy()?
                .execute_command(&task.command)
                .await
                .map_err(client_error),
            Err(error) => Err(error),
        };
        response::commit(&owner, &task, result)
    }));
    let (reply, receive) = tokio::sync::oneshot::channel();
    executor.spawn(async move {
        let _ = reply.send(worker.await.unwrap_or(Err(PANIC)));
    });
    watch(executor, mobile, operation, receive);
    Ok(id)
}
