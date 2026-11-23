use super::{PeerState, Phase, RemotePeer, Uuid};
use shellfox_service::remote::native::RemoteConnection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionAction {
    Connect,
    Retry,
    Cancel(Uuid),
    Disconnect,
}

impl PeerState {
    pub fn creation_connection_failed(&self) -> bool {
        !self.pending
            && (self.phase == Phase::Rejected || self.phase == Phase::Disconnected && !self.online)
    }

    pub fn connection_action(&self) -> Option<ConnectionAction> {
        if self.pending {
            return None;
        }
        if self.phase == Phase::Connected {
            return Some(ConnectionAction::Disconnect);
        }
        if let Some(id) = self.owner_pair_attempt {
            return Some(ConnectionAction::Cancel(id));
        }
        match self.phase {
            Phase::Connecting | Phase::AwaitingApproval => None,
            Phase::Rejected if self.online => Some(ConnectionAction::Retry),
            _ if self.online => Some(ConnectionAction::Connect),
            _ => None,
        }
    }

    pub(super) fn observe_owner(&mut self, peer: &RemotePeer) {
        if self.pending {
            return;
        }
        self.owner_connected = peer.connection == RemoteConnection::Connected;
        if self.phase == Phase::Connected {
            return;
        }
        self.owner_pair_attempt = match peer.connection {
            RemoteConnection::Connecting
            | RemoteConnection::Reconnecting
            | RemoteConnection::AwaitingApproval
            | RemoteConnection::Disconnected => peer.attempt_id,
            _ => None,
        };
        if self.stopped_pair_attempt != self.owner_pair_attempt {
            self.stopped_pair_attempt = None;
        }
        self.phase = match peer.connection {
            RemoteConnection::Connecting => Phase::Connecting,
            RemoteConnection::AwaitingApproval => Phase::AwaitingApproval,
            RemoteConnection::Rejected => Phase::Rejected,
            RemoteConnection::Reconnecting => Phase::Reconnecting,
            RemoteConnection::Disconnected => Phase::Disconnected,
            // The owner is connected, but the workspace still needs its catalog.
            RemoteConnection::Connected if self.phase == Phase::AwaitingApproval => {
                Phase::Connecting
            }
            RemoteConnection::Connected => return,
        };
        self.error = peer.last_error.as_ref().map(|error| error.message.clone());
    }
}
