//! Closed private wire routes. Native registration/claim/ACK are not wire DTOs.
use super::*;
use crate::desktop_launch::DesktopLaunchRequest;
use shellfox_protocol::{automation::*, LocalRequest};

pub(crate) enum PrivateRequest {
    Peer(crate::remote::outgoing_wire::Request),
    RemoteOperator(crate::remote::RemoteRequest),
    LocalApi(LocalApiRequest),
    DesktopView(Box<super::DesktopRequest>),
    SshRead(super::SshRead),
    Power(crate::power::PowerRequest),
    AutomationStart(Box<AutomationStart>),
    AutomationList(AutomationList),
    AutomationRead(AutomationRead),
    AutomationWrite(AutomationWrite),
    AutomationStop(AutomationStop),
    AutomationShow(AutomationShow),
    DesktopLaunch(DesktopLaunchRequest),
}
impl PrivateRequest {
    pub(crate) fn decode(bytes: &[u8]) -> Result<(uuid::Uuid, Self)> {
        // Inspect only routing first, then deserialize the original bytes into
        // the exact typed payload. An intermediate Value would erase duplicate
        // environment keys before AutomationStart's validator could reject them.
        let header: Envelope<serde::de::IgnoredAny> = serde_json::from_slice(bytes)?;
        if header.client_id.is_nil() {
            bail!("A non-nil private client ID is required.");
        }
        let request = match header.method.as_str() {
            METHOD => Self::LocalApi(payload::<LocalApiRequest>(bytes)?),
            super::DesktopRequest::METHOD => Self::DesktopView(Box::new(payload(bytes)?)),
            super::SshRead::METHOD => Self::SshRead(payload(bytes)?),
            crate::remote::outgoing_wire::METHOD => Self::Peer(payload(bytes)?),
            crate::remote::operator::METHOD => Self::RemoteOperator(payload(bytes)?),
            crate::power::PowerRequest::METHOD => Self::Power(payload(bytes)?),
            AutomationStart::METHOD => Self::AutomationStart(Box::new(payload(bytes)?)),
            AutomationList::METHOD => Self::AutomationList(payload(bytes)?),
            AutomationRead::METHOD => Self::AutomationRead(payload(bytes)?),
            AutomationWrite::METHOD => Self::AutomationWrite(payload(bytes)?),
            AutomationStop::METHOD => Self::AutomationStop(payload(bytes)?),
            AutomationShow::METHOD => Self::AutomationShow(payload(bytes)?),
            shellfox_protocol::DESKTOP_LAUNCH_NAMESPACE => Self::DesktopLaunch(payload(bytes)?),
            _ => bail!("Unsupported private method."),
        };
        Ok((header.client_id, request))
    }
    pub(crate) fn requested_stop(&self) -> bool {
        matches!(self, Self::LocalApi(LocalApiRequest::Stop {}))
    }
}
fn payload<T: serde::de::DeserializeOwned + Validate>(bytes: &[u8]) -> Result<T> {
    let envelope: Envelope<T> = serde_json::from_slice(bytes)?;
    envelope.payload.validate()?;
    Ok(envelope.payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn envelope(method: &str, payload: &str) -> String {
        format!(
            r#"{{"clientId":"12345678-1234-4234-8234-123456789abc","method":"{method}","payload":{payload}}}"#
        )
    }
    #[test]
    fn exact_closed_method_table() {
        for method in [
            "automation.unknown",
            "automation",
            "private.other.v1",
            "getSnapshot",
            "desktop.register",
        ] {
            assert!(PrivateRequest::decode(envelope(method, "{}").as_bytes()).is_err());
        }
        assert!(matches!(
            PrivateRequest::decode(envelope(METHOD, r#"{"operation":"snapshot"}"#).as_bytes()),
            Ok((_, PrivateRequest::LocalApi(_)))
        ));
        assert!(matches!(
            PrivateRequest::decode(envelope(AutomationList::METHOD, "{}").as_bytes()),
            Ok((_, PrivateRequest::AutomationList(_)))
        ));
        assert!(matches!(
            PrivateRequest::decode(
                envelope(
                    shellfox_protocol::DESKTOP_LAUNCH_NAMESPACE,
                    r#"{"operation":"discover"}"#
                )
                .as_bytes()
            ),
            Ok((_, PrivateRequest::DesktopLaunch(_)))
        ));
    }
    #[test]
    fn ssh_profiles_are_read_only_and_require_one_non_nil_id_for_secrets() {
        let method = super::super::SshRead::METHOD;
        assert!(matches!(
            PrivateRequest::decode(envelope(method, r#"{"action":"list"}"#).as_bytes()),
            Ok((_, PrivateRequest::SshRead(super::super::SshRead::List {})))
        ));
        for bad in [
            r#"{"action":"save"}"#,
            r#"{"action":"connection"}"#,
            r#"{"action":"connection","id":"00000000-0000-0000-0000-000000000000"}"#,
            r#"{"action":"list","includeSecrets":true}"#,
        ] {
            assert!(PrivateRequest::decode(envelope(method, bad).as_bytes()).is_err());
        }
    }
    #[test]
    fn duplicate_environment_is_not_erased_by_route_inspection() {
        let body = envelope(
            AutomationStart::METHOD,
            r#"{"requestId":"12345678-1234-4234-8234-123456789abc","executable":"test","args":[],"cwd":"/tmp","env":{"KEY":"first","KEY":"second"},"detached":false}"#,
        );
        assert!(PrivateRequest::decode(body.as_bytes()).is_err());
    }
    #[test]
    fn native_capabilities_cannot_cross_the_private_namespace() {
        for operation in ["register", "claim", "complete", "unregister"] {
            let payload = format!(r#"{{"operation":"{operation}"}}"#);
            assert!(PrivateRequest::decode(
                envelope(shellfox_protocol::DESKTOP_LAUNCH_NAMESPACE, &payload).as_bytes()
            )
            .is_err());
        }
        assert!(PrivateRequest::decode(
            envelope(AutomationList::METHOD, r#"{"unexpected":true}"#).as_bytes()
        )
        .is_err());
    }
}
