use super::control::revoke;
use super::*;
pub(super) fn poll(
    state: &mut RemoteState,
    service: &mut Service,
    id: m::Id,
) -> Result<serde_json::Value, AppError> {
    let revision = state
        .metadata_v2
        .get(&id)
        .ok_or_else(native_error)?
        .revision;
    let mut events = Vec::new();
    if let Some(mut event) = service.host().changed_since(revision).map_err(host_error)? {
        event.select_session_id = None;
        let wire::HostEvent::Changed { event } =
            adapter::changed_event(&event).map_err(AppError::from)?
        else {
            return Err(native_error());
        };
        events.push(m::HostEvent::Changed { event });
    }
    let sources = state
        .manager_views
        .entries
        .get(&id)
        .ok_or_else(native_error)?
        .sources
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let mut budget = m::MAX_POLL_DATA_BYTES;
    for source in sources {
        if let Some(reason) = service.manager_v2_mailboxes.revocation(&source.delivery) {
            revoke(state, service, id, &source, reason)?;
            events.push(m::HostEvent::Terminal {
                event: m::Event::Revoked {
                    delivery: source.delivery.clone(),
                    reason,
                },
            });
            continue;
        }
        if !allowed(state, service, &source) {
            revoke(state, service, id, &source, m::RevokedReason::PolicyChanged)?;
            events.push(m::HostEvent::Terminal {
                event: m::Event::Revoked {
                    delivery: source.delivery.clone(),
                    reason: m::RevokedReason::PolicyChanged,
                },
            });
            continue;
        }
        service
            .drain(source.identity.tab_id)
            .map_err(|_| native_error())?;
        if !allowed(state, service, &source) {
            revoke(state, service, id, &source, m::RevokedReason::PolicyChanged)?;
            events.push(m::HostEvent::Terminal {
                event: m::Event::Revoked {
                    delivery: source.delivery.clone(),
                    reason: m::RevokedReason::PolicyChanged,
                },
            });
            continue;
        }
        if let Some(reason) = service.manager_v2_mailboxes.revocation(&source.delivery) {
            revoke(state, service, id, &source, reason)?;
            events.push(m::HostEvent::Terminal {
                event: m::Event::Revoked {
                    delivery: source.delivery.clone(),
                    reason,
                },
            });
            continue;
        }
        let book = service
            .manager_v2_mailboxes
            .get(source.identity, source.role)
            .ok_or_else(|| {
                failure(
                    ErrorCode::TargetLost,
                    "Manager delivery retired; acquire fresh references and recovery.",
                )
            })?;
        for event in book
            .poll(budget, 1024usize.saturating_sub(events.len()))
            .map_err(|_| {
                failure(
                    ErrorCode::TargetLost,
                    "Manager replay gap requires fresh attachment/recovery.",
                )
            })?
        {
            budget = budget.saturating_sub(event.decoded_bytes().map_err(|_| native_error())?);
            events.push(m::HostEvent::Terminal { event });
        }
    }
    checked(m::PollResponse { events })
}
