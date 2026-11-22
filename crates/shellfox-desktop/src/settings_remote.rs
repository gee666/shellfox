use super::*;
#[derive(Clone)]
pub(super) enum Operation {
    ToggleEnabled,
    SaveHostname,
    Login,
    CancelLogin,
    OpenLogin,
    Advanced,
    UseKey,
    Connect(String),
    CancelAttempt(String),
    CancelPair(String),
    Disconnect(String),
    Approve(String),
    Reject(String),
    Forget(String),
    Revoke(String),
    ConfirmRevoke(String),
    CancelRevoke,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ErrorSection {
    Access,
    Machines,
    Clients,
}

impl Operation {
    pub fn client_access_target(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::Approve(id) => Some(("Approve", id)),
            Self::Reject(id) => Some(("Reject", id)),
            Self::Revoke(id) => Some(("Revoke", id)),
            Self::ConfirmRevoke(id) => Some(("Confirm revoke", id)),
            _ => None,
        }
    }
    pub fn section(&self) -> ErrorSection {
        match self {
            Self::Connect(_) | Self::CancelPair(_) | Self::Disconnect(_) | Self::Forget(_) => {
                ErrorSection::Machines
            }
            Self::Approve(_) | Self::Reject(_) | Self::ConfirmRevoke(_) => ErrorSection::Clients,
            _ => ErrorSection::Access,
        }
    }
}

pub(super) struct RemoteView {
    pub state: Option<api::RemoteState>,
    pub hostname: Entity<Field>,
    pub auth: Entity<Field>,
    pub login_address: Entity<Field>,
    pub pending: bool,
    pub login_request: bool,
    pub login_attempt: Option<String>,
    pub read_error: Option<String>,
    pub error: Option<(ErrorSection, String)>,
    pub advanced: bool,
    pub revoking: Option<String>,
    pub epoch: u64,
    pub login: login::Login,
    pub login_started: Option<std::time::Instant>,
    pub reading: bool,
    pub polling: bool,
    pub connecting_peer: Option<String>,
    pub pairing_scope: pairing::Scope,
}
impl RemoteView {
    pub fn new(cx: &mut App) -> Self {
        let hostname = Field::settings_input(None, cx);
        hostname.update(cx, |field, _| field.limit_utf16(63));
        let view = Self {
            state: None,
            hostname,
            auth: Field::password(cx),
            login_address: Field::new("", cx),
            pending: false,
            login_request: false,
            login_attempt: None,
            read_error: None,
            error: None,
            advanced: false,
            revoking: None,
            epoch: 0,
            login: login::Login::new(),
            login_started: None,
            reading: false,
            polling: false,
            connecting_peer: None,
            pairing_scope: Default::default(),
        };
        view.auth.update(cx, |field, cx| {
            field.configure(
                crate::field::FieldOptions {
                    placeholder: Some("tskey-auth-…".into()),
                    ..Default::default()
                },
                cx,
            )
        });
        view.login_address
            .update(cx, |field, cx| field.set_read_only(true, cx));
        view
    }

    pub fn connected(&self, id: &str) -> bool {
        self.state.as_ref().is_some_and(|state| {
            state.peers.iter().any(|peer| {
                peer.id == id && peer.connection == api::Connection::Connected
            })
        })
    }

    pub fn retry_release_id(&self) -> Option<&str> {
        if self.login.phase != login::Phase::Error {
            return None;
        }
        let id = self.login_attempt.as_deref()?;
        self.state
            .as_ref()?
            .attempts
            .iter()
            .any(|attempt| {
                attempt.id == id && matches!(attempt.kind, api::RemoteAttemptKind::Login)
            })
            .then_some(id)
    }

    pub fn client_access_name(&self, operation: &Operation) -> Option<&str> {
        let (_, id) = operation.client_access_target()?;
        let state = self.state.as_ref()?;
        if matches!(operation, Operation::Approve(_) | Operation::Reject(_)) {
            state
                .pending
                .iter()
                .find(|peer| peer.id == id)
                .map(|peer| peer.hostname.as_str())
        } else {
            state
                .approvals
                .iter()
                .find(|peer| peer.peer_id == id)
                .map(|peer| peer.hostname.as_str())
        }
    }

    pub fn error_at(&self, section: ErrorSection) -> Option<String> {
        self.error
            .as_ref()
            .filter(|(owner, _)| *owner == section)
            .map(|(_, message)| message.clone())
    }

    pub fn fail(&mut self, section: ErrorSection, message: &str) {
        let text = message.split('\n').next().unwrap_or("Request failed.");
        self.error = Some((section, text.chars().take(200).collect()));
    }
}
pub(super) fn connection(peer: &api::Peer) -> &'static str {
    if !peer.online && peer.connection == api::Connection::Disconnected {
        return "Offline";
    }
    match peer.connection {
        api::Connection::Disconnected => "Disconnected",
        api::Connection::Connecting => "Connecting…",
        api::Connection::Reconnecting => "Reconnecting…",
        api::Connection::Connected => "Connected",
        api::Connection::AwaitingApproval => "Waiting for approval",
        api::Connection::Rejected => "Request declined",
    }
}
