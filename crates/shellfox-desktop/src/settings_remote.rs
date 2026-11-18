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
    ContinueAttempt(String),
    Disconnect(String),
    Approve(String),
    Reject(String),
    Forget(String),
    Revoke(String),
    ConfirmRevoke(String),
    CancelRevoke,
}
pub(super) struct RemoteView {
    pub state: Option<api::RemoteState>,
    pub hostname: Entity<Field>,
    pub auth: Entity<Field>,
    pub login_address: Entity<Field>,
    pub pending: bool,
    pub login_request: bool,
    pub error: Option<String>,
    pub advanced: bool,
    pub revoking: Option<String>,
    pub epoch: u64,
    pub login: login::Login,
    pub login_started: Option<std::time::Instant>,
    pub reading: bool,
    pub polling: bool,
    pub inflight_peer: Option<String>,
}
impl RemoteView {
    pub fn new(cx: &mut App) -> Self {
        let view = Self {
            state: None,
            hostname: Field::new("", cx),
            auth: Field::password(cx),
            login_address: Field::new("", cx),
            pending: false,
            login_request: false,
            error: None,
            advanced: false,
            revoking: None,
            epoch: 0,
            login: login::Login::new(),
            login_started: None,
            reading: false,
            polling: false,
            inflight_peer: None,
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
