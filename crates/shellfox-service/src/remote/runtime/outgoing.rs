use super::*;
use crate::remote::outgoing::{OutgoingEnvelope, OutgoingFence};
use wire::TransportError;
/// Peer requests in flight at once, across all peers.
pub(super) const MAX_OUTGOING: usize = 16;
impl Manager {
    pub(super) fn cancel_outgoing_peer(&mut self, peer: &str) {
        for fence in self
            .outgoing_fences
            .iter()
            .filter_map(std::sync::Weak::upgrade)
        {
            if fence.peer == peer {
                fence.cancel.cancel();
            }
        }
    }
    pub(super) fn prune_outgoing(&mut self) {
        self.outgoing_fences.retain(|weak| {
            let Some(fence) = weak.upgrade() else {
                return false;
            };
            if !self.state.enabled
                || self.state.cleanup_retained
                || !self
                    .state
                    .peers
                    .iter()
                    .any(|peer| peer.id == fence.peer && peer.online)
            {
                fence.cancel.cancel();
            }
            true
        });
    }
    pub(super) fn outgoing_request(&mut self, envelope: OutgoingEnvelope) {
        if envelope.reply.is_closed() {
            return;
        }
        let result = (|| {
            let limits =
                crate::remote::outgoing::validate(&envelope.body, envelope.limits.clone())?;
            if !self.state.enabled
                || self.state.cleanup_retained
                || !self
                    .state
                    .peers
                    .iter()
                    .any(|p| p.id == envelope.peer.as_str() && p.online)
            {
                return Err(TransportError::Unavailable);
            }
            Ok((
                self.network.clone().ok_or(TransportError::Unavailable)?,
                limits,
            ))
        })();
        let (network, limits) = match result {
            Ok(value) => value,
            Err(error) => {
                let _ = envelope.reply.send(Err(error));
                return;
            }
        };
        let fence = Arc::new(OutgoingFence {
            peer: envelope.peer.as_str().into(),
            cancel: CancellationToken::new(),
        });
        self.outgoing_fences.push(Arc::downgrade(&fence));
        let cycle = self.cycle.clone();
        self.jobs.spawn(async move {
            let OutgoingEnvelope {
                peer,
                endpoint,
                body,
                mut reply,
                ..
            } = envelope;
            let cancel = limits.cancel.clone();
            let deadline = limits.deadline;
            let maximum = limits.max_response_bytes;
            // This owns only a bounded HTTP future and the ORIGINAL network Arc.
            // The Manager remains the helper/controller owner through shutdown.
            let result = tokio::select! {
                biased;
                _=reply.closed()=>return,
                _=cycle.cancelled()=>Err(TransportError::Unavailable),
                _=fence.cancel.cancelled()=>Err(TransportError::Unavailable),
                _=cancel.cancelled()=>Err(TransportError::Cancelled),
                _=tokio::time::sleep_until(deadline.into())=>Err(TransportError::Deadline),
                result=network.peer_request_with_status(&peer, endpoint, body, limits)=>result,
            };
            let result = result.and_then(|response| {
                crate::remote::outgoing_wire::check_response(&response, maximum)?;
                Ok(response)
            });
            let _ = reply.send(result);
        });
    }
}
