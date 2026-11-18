//! Thin frontend adapter over published owner-routed capabilities only.
use super::{api, native_gateway::Gateway};
use crate::host_client::HostClient;
use shellfox_service::remote::native::{AuthKey, RemoteRequest};
use std::sync::Arc;
pub(super) struct NativeServices {
    gateway: Arc<dyn Gateway>,
}
impl NativeServices {
    pub fn new(client: HostClient) -> Self {
        Self {
            gateway: Arc::new(client),
        }
    }
    #[cfg(test)]
    pub(super) fn injected(gateway: Arc<dyn Gateway>) -> Self {
        Self { gateway }
    }
}
impl api::SettingsServices for NativeServices {
    fn request(&self, request: api::Request) -> api::Reply<'_> {
        Box::pin(async move {
            if matches!(
                &request,
                api::Request::SshList
                    | api::Request::SshSave(_)
                    | api::Request::SshDelete(_)
                    | api::Request::SshImport
                    | api::Request::ValidateSshKey(_)
            ) {
                let bound = super::native_ssh::bind(request)?;
                let response = self.gateway.execute(bound.action).await?;
                return super::native_ssh::project(bound.expected, response);
            }
            let request = bind(request)?;
            let response = self
                .gateway
                .execute(shellfox_service::native::NativeAction::Remote(request))
                .await?;
            match response {
                shellfox_service::native::NativeResponse::Remote(state) => {
                    Ok(api::Response::Remote(super::native_state::project(state)))
                }
                _ => anyhow::bail!("Invalid typed remote-settings response from the shared host."),
            }
        })
    }
}
pub(super) fn bind(request: api::Request) -> anyhow::Result<RemoteRequest> {
    let request = match request {
        api::Request::RemoteState => RemoteRequest::State,
        api::Request::ConfigureRemote { enabled, hostname } => {
            RemoteRequest::Configure { enabled, hostname }
        }
        api::Request::Login { auth_key } => RemoteRequest::Login {
            auth_key: auth_key
                .map(AuthKey::new)
                .transpose()
                .map_err(|error| anyhow::anyhow!(error.message))?,
        },
        api::Request::Connect(peer_id) => RemoteRequest::Connect { peer_id },
        api::Request::CancelAttempt(id) => RemoteRequest::Cancel {
            attempt_id: id
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid remote attempt ID."))?,
        },
        api::Request::ContinueAttempt(id) => RemoteRequest::Continue {
            attempt_id: id
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid remote attempt ID."))?,
        },
        api::Request::Disconnect(peer_id) => RemoteRequest::Disconnect { peer_id },
        api::Request::Reject(peer_id) => RemoteRequest::Reject { peer_id },
        api::Request::Revoke(peer_id) => RemoteRequest::Revoke { peer_id },
        api::Request::Forget(peer_id) => RemoteRequest::Forget { peer_id },
        api::Request::Approve(peer_id) => RemoteRequest::Approve { peer_id },
        api::Request::SshList
        | api::Request::SshSave(_)
        | api::Request::SshDelete(_)
        | api::Request::SshImport
        | api::Request::ValidateSshKey(_) => {
            anyhow::bail!("SSH requests require the typed SSH response path.")
        }
    };
    request
        .validate()
        .map_err(|error| anyhow::anyhow!(error.message))?;
    Ok(request)
}
