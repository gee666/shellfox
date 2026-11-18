use super::*;
use serde::{de::DeserializeOwned, Serialize};

fn typed<T: DeserializeOwned + Validate>(value: &serde_json::Value) -> Result<T> {
    let value: T = serde_json::from_value(value.clone()).map_err(|_| PROTOCOL)?;
    value.validate().map_err(|_| PROTOCOL)?;
    Ok(value)
}
fn json(value: impl Serialize) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(|_| PROTOCOL)
}
pub(super) fn commit(
    mobile: &Mobile,
    task: &UploadTask,
    result: Result<Reply<serde_json::Value>>,
) -> Result<serde_json::Value> {
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    let current = !s.closed
        && s.generation == task.operation.generation
        && !task.operation.cancel.is_cancelled();
    let b = s.bindings.get_mut(&task.tab).ok_or(STALE)?;
    let current = current && b.uploads.serial == task.serial && b.connection == task.connection;
    if let RemoteCommand::BeginTerminalUpload(begin) = &task.command {
        if b.uploads.begin_active.as_ref().map(|op| op.id) != Some(task.operation.id) {
            return Err(STALE);
        }
        b.uploads.begin_active = None;
        if !current {
            return Err(STALE);
        }
        if let Ok(Reply::Success(value)) = &result {
            let started: TerminalUploadStarted = typed(value)?;
            if b.uploads.records.contains_key(&started.upload_id) {
                return Err(PROTOCOL);
            }
            let view = b.cursor.as_ref().ok_or(STALE)?;
            let upload_id = started.upload_id;
            let cursor = UploadCursor::new(begin.clone(), started, view, task.connection)
                .map_err(upload_error)?;
            b.uploads.records.insert(
                upload_id,
                UploadRecord {
                    cursor,
                    begin: begin.clone(),
                    connection: task.connection,
                    active: None,
                },
            );
        }
        return json(result?);
    }
    let id = match &task.command {
        RemoteCommand::WriteTerminalUpload(v) => v.upload_id,
        RemoteCommand::FinishTerminalUpload(v) => v.upload_id,
        RemoteCommand::CancelTerminalUpload(v) => v.upload_id,
        _ => return Err(INVALID),
    };
    let view = b.cursor.clone();
    let record = b.uploads.records.get_mut(&id).ok_or(STALE)?;
    if record.active.as_ref().map(|op| op.id) != Some(task.operation.id) {
        return Err(STALE);
    }
    record.active = None;
    if !current {
        record.cursor.failed();
        return Err(STALE);
    }
    let result = match result {
        Ok(Reply::Success(value)) => value,
        Ok(Reply::Failure(error)) => {
            record.cursor.failed();
            return json(Reply::<serde_json::Value>::Failure(error));
        }
        Err(error) => {
            record.cursor.failed();
            return Err(error);
        }
    };
    let checked = match &task.command {
        RemoteCommand::WriteTerminalUpload(_) => {
            let response: TransferOffset = typed(&result)?;
            record
                .cursor
                .written(&response, view.as_ref().ok_or(STALE)?, task.connection)
                .map(|_| ())
                .map_err(upload_error)
        }
        RemoteCommand::FinishTerminalUpload(_) => {
            let response: TerminalUploadResult = typed(&result)?;
            record.cursor.finished(&response).map_err(upload_error)
        }
        RemoteCommand::CancelTerminalUpload(_) => typed::<Cancelled>(&result).map(|_| ()),
        _ => Err(INVALID),
    };
    if let Err(error) = checked {
        record.cursor.failed();
        return Err(if error == INVALID { PROTOCOL } else { error });
    }
    if matches!(
        task.command,
        RemoteCommand::FinishTerminalUpload(_) | RemoteCommand::CancelTerminalUpload(_)
    ) {
        b.uploads.records.remove(&id);
    }
    json(Reply::Success(result))
}
