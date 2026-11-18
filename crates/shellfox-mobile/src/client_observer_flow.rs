use super::{observer_delivery as delivery, runtime::*};
use shellfox_client::observer_v2::{CursorError, DeliveryCursor, EventDisposition};
use shellfox_protocol::observer_v2 as v2;
use std::sync::Arc;

pub(super) fn attachment(
    mobile: &Mobile,
    tab: v2::Id,
    value: v2::Attachment,
) -> Result<serde_json::Value> {
    let binding = mobile.observer()?.binding().ok_or(STALE)?;
    let operation = CURRENT.try_with(Clone::clone).map_err(|_| STALE)?;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed || s.generation != operation.generation || operation.cancel.is_cancelled() {
        return Err(STALE);
    }
    let cached: usize = s
        .observers
        .values()
        .filter_map(|b| b.cursor.as_ref())
        .map(|c| c.recovery().data.len())
        .sum();
    let cursor = match &value {
        v2::Attachment::Attached {
            delivery, recovery, ..
        } => {
            if delivery.tab_id != tab {
                return Err(PROTOCOL);
            }
            crate::engine::dimensions(recovery.cols, recovery.rows)
                .map_err(|status| status as i32)?;
            if cached + recovery.data.len() > 16 * 1024 * 1024 {
                return Err(BUSY);
            }
            Some(DeliveryCursor::new(binding, &value).map_err(|_| PROTOCOL)?)
        }
        v2::Attachment::Unavailable { tab_id, .. } => {
            if *tab_id != tab {
                return Err(PROTOCOL);
            }
            None
        }
    };
    let ticket = s.next_reset_ticket;
    s.next_reset_ticket = ticket.checked_add(1).ok_or(BUSY)?;
    let b = s.observers.get_mut(&tab).ok_or(STALE)?;
    if b.attaching != Some(operation.id) {
        return Err(STALE);
    }
    b.clear();
    b.attaching = Some(operation.id);
    b.cursor = cursor;
    b.reset_ticket = ticket;
    b.reset_required = true;
    b.required_grid = b
        .cursor
        .as_ref()
        .map(|c| (c.recovery().cols, c.recovery().rows));
    let mut metadata = serde_json::to_value(v2::Reply::Success(value)).map_err(|_| PROTOCOL)?;
    if let Some(recovery) = metadata
        .get_mut("value")
        .and_then(|v| v.get_mut("recovery"))
        .and_then(serde_json::Value::as_object_mut)
    {
        recovery.remove("data");
    }
    Ok(metadata)
}
pub(super) fn poll(mobile: Arc<Mobile>) -> Result<u64> {
    delivery::poll_ready(&mobile)?;
    launch(
        mobile,
        CompletionOrigin::NativeInternal,
        |mobile| async move {
            match mobile
                .observer()?
                .poll()
                .await
                .map_err(super::observer::observer_error)?
            {
                v2::Reply::Success(response) => events(&mobile, response),
                v2::Reply::Failure(error) => {
                    serde_json::to_value(v2::Reply::<v2::PollResponse>::Failure(error))
                        .map_err(|_| PROTOCOL)
                }
            }
        },
    )
}
fn events(mobile: &Mobile, response: v2::PollResponse) -> Result<serde_json::Value> {
    let current = mobile.observer()?.binding().ok_or(STALE)?;
    let op = CURRENT.try_with(Clone::clone).map_err(|_| STALE)?;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed || s.generation != op.generation || op.cancel.is_cancelled() {
        return Err(STALE);
    }
    let result = (|| {
        let mut coarse = Vec::new();
        for event in response.events {
            match event {
                v2::HostEvent::Changed { event } => {
                    coarse.push(serde_json::json!({"type":"changed","event":event}))
                }
                v2::HostEvent::Terminal { event } => {
                    let ticket = s.next_reset_ticket;
                    s.next_reset_ticket = ticket.checked_add(1).ok_or(BUSY)?;
                    let Some(b) = s.observers.get_mut(&event.delivery().tab_id) else {
                        continue;
                    };
                    let Some(cursor) = b.cursor.as_mut() else {
                        continue;
                    };
                    let disposition = match cursor.observe(&event, &current) {
                        Ok(value) => value,
                        Err(CursorError::StaleDelivery) => continue,
                        Err(_) => return Err(PROTOCOL),
                    };
                    match disposition {
                        EventDisposition::Ignore => continue,
                        EventDisposition::Reattach => {
                            let request = cursor.recovery_request(&current).map_err(|_| STALE)?;
                            b.clear();
                            b.reset_required = true;
                            b.reset_ticket = ticket;
                            coarse.push(serde_json::json!({"type":"reattach","request":request}));
                            continue;
                        }
                        EventDisposition::Revoked => {
                            b.clear();
                            b.reset_required = true;
                            b.reset_ticket = ticket;
                        }
                        EventDisposition::Deliver => {}
                    }
                    if let v2::Event::Data { sequence, data, .. } = event {
                        if b.reset_required || !b.recovery_acked {
                            return Err(PROTOCOL);
                        }
                        let bytes =
                            v2::decode_bytes(&data, v2::MAX_DATA_BYTES).map_err(|_| PROTOCOL)?;
                        b.pending.push_back(delivery::Segment {
                            bytes,
                            sequence: Some(sequence),
                        });
                        delivery::flush(b)?;
                    } else {
                        coarse.push(serde_json::json!({"type":"terminal","event":event}));
                    }
                }
            }
        }
        Ok(serde_json::json!({"version":2,"ok":true,"value":{"events":coarse}}))
    })();
    if result.is_err() {
        delivery::fence(&mut s)?;
    }
    result
}
pub(super) fn command(mobile: Arc<Mobile>, command: v2::Command) -> Result<u64> {
    mobile.observer()?;
    shellfox_protocol::Validate::validate(&command).map_err(|_| INVALID)?;
    command.authorize_profile().map_err(|_| INVALID)?;
    if mobile.state.lock().map_err(|_| PANIC)?.observer_withdrawing {
        return Err(STALE);
    }
    match command {
        v2::Command::AcknowledgeTerminal(_) => Err(INVALID),
        v2::Command::AttachTerminal(request) => delivery::attach(mobile, request),
        v2::Command::DetachTerminal(request) => {
            let current = mobile.observer()?.binding().ok_or(STALE)?;
            {
                let s = mobile.state.lock().map_err(|_| PANIC)?;
                let cursor = s
                    .observers
                    .get(&request.delivery.tab_id)
                    .and_then(|b| b.cursor.as_ref())
                    .ok_or(STALE)?;
                if cursor.detach_request(&current).map_err(|_| STALE)?.delivery != request.delivery
                {
                    return Err(STALE);
                }
            }
            launch(mobile, CompletionOrigin::Explicit, |mobile| async move {
                let _slot = request_slot(&mobile).await?;
                let result = mobile
                    .observer()?
                    .execute(&request)
                    .await
                    .map_err(super::observer::observer_error)?;
                if matches!(result, v2::Reply::Success(_)) {
                    let op = CURRENT.try_with(Clone::clone).map_err(|_| STALE)?;
                    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
                    if s.closed || s.generation != op.generation || op.cancel.is_cancelled() {
                        return Err(STALE);
                    }
                    if s.observers
                        .get(&request.delivery.tab_id)
                        .and_then(|b| b.cursor.as_ref())
                        .map(|c| c.delivery())
                        != Some(&request.delivery)
                    {
                        return Err(STALE);
                    }
                    let ticket = s.next_reset_ticket;
                    s.next_reset_ticket = ticket.checked_add(1).ok_or(BUSY)?;
                    if let Some(b) = s.observers.get_mut(&request.delivery.tab_id) {
                        b.clear();
                        b.reset_required = true;
                        b.reset_ticket = ticket;
                    }
                }
                serde_json::to_value(result).map_err(|_| PROTOCOL)
            })
        }
        command => launch(mobile, CompletionOrigin::Explicit, |mobile| async move {
            let _slot = request_slot(&mobile).await?;
            let result = mobile
                .observer()?
                .execute_command(&command)
                .await
                .map_err(super::observer::observer_error)?;
            serde_json::to_value(result).map_err(|_| PROTOCOL)
        }),
    }
}
