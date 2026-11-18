//! Canonical generic-client tests, not authenticated Go or Runtime grant proof.
use super::*;
use shellfox_client::manager_v2::{ManagerClient, Status};
use shellfox_protocol::{
    AuthenticatedPeerId, Endpoint, OutboundTransport, RequestLimits, TransportFuture,
    manager_v2 as m,
};
use std::sync::{Arc, Mutex};
#[derive(Clone, Default)]
struct Memory(Arc<Mutex<Vec<String>>>);
impl OutboundTransport for Memory {
    fn send<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        _: RequestLimits,
    ) -> TransportFuture<'a> {
        assert_eq!(peer.as_str(), "n-memory-peer");
        Box::pin(async move {
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let value = match endpoint {
                Endpoint::DiscoverV2 => serde_json::from_str::<serde_json::Value>(include_str!(
                    "../../shellfox-protocol/tests/fixtures/manager_v2/discovery.json"
                ))
                .unwrap(),
                Endpoint::PairV2 => {
                    self.0.lock().unwrap().push("PAIR".into());
                    let offer: m::PairOffer = serde_json::from_value(body).unwrap();
                    serde_json::json!({"version":2,"ok":true,"value":m::PairSelection{client_id:offer.client_id,attempt_id:offer.attempt_id,owner_id:offer.owner_id,connection_id:m::Id::new(),profile:offer.profile,catalog_mode:offer.catalog_mode,delivery_format:offer.delivery_format,recovery_format:offer.recovery_format,input_format:offer.input_format,device_policy:m::DevicePolicy::ControlFullAccount,device_permission:m::DevicePermission::Write,native0_sender:None,native0_receiver:None}})
                }
                Endpoint::RequestV2 => {
                    let method = body["method"].as_str().unwrap();
                    self.0.lock().unwrap().push(method.into());
                    let response = match method {
                        "getHomeDirectory" => serde_json::json!({"cwd":"/peer/home"}),
                        "getSnapshot" => {
                            let raw: serde_json::Value = serde_json::from_str(include_str!(
                                "../../shellfox-protocol/tests/fixtures/observer_v2/snapshot.json"
                            ))
                            .unwrap();
                            raw["value"].clone()
                        }
                        _ => panic!("Unexpected operation {method}"),
                    };
                    serde_json::json!({"version":2,"binding":body["binding"],"ok":true,"value":response})
                }
                Endpoint::DisconnectV2 => {
                    serde_json::json!({"version":2,"ok":true,"value":{"disconnected":true}})
                }
                _ => panic!("No legacy/network/fallback"),
            };
            Ok(serde_json::to_vec(&value).unwrap())
        })
    }
}
#[tokio::test]
async fn generic_original_admission_commits_once_and_cannot_rebind_after_reconnect() {
    let wire = Memory::default();
    let id = m::Id::new();
    let peer = AuthenticatedPeerId::new("n-memory-peer").unwrap();
    let client = Arc::new(ManagerClient::new(peer.clone(), id, wire.clone()));
    let connection = PeerConnection::new(client, id);
    let origin = connection.connect().await.unwrap();
    assert!(connection.is_current(&origin));
    assert_eq!(connection.status(), Status::Connected);
    let second = connection.connect().await.unwrap();
    assert!(second.witness.same_origin(&origin.witness));
    assert_eq!(
        wire.0
            .lock()
            .unwrap()
            .iter()
            .filter(|method| method.as_str() == "PAIR")
            .count(),
        1
    );
    let admission: SourceAdmission = connection.admit_source_with(&origin.witness).unwrap();
    assert_eq!(admission.peer(), &peer);
    admission.check().unwrap();
    let home = connection
        .execute_admitted(
            &admission,
            &m::GetHomeDirectory(shellfox_protocol::GetHomeDirectory {}),
        )
        .await
        .unwrap();
    assert_eq!(home.cwd, "/peer/home");
    let mut applied = false;
    admission.commit_if_current(|| applied = true).unwrap();
    assert!(applied);
    let snapshot = connection.snapshot(&origin).await.unwrap();
    let tab = snapshot
        .entries
        .iter()
        .find_map(|entry| match entry {
            m::CatalogEntry::Session { value } => value.tabs.first().map(|tab| tab.id),
            _ => None,
        })
        .unwrap();
    let expected = snapshot
        .entries
        .iter()
        .find_map(|entry| match entry {
            m::CatalogEntry::Session { value } if value.tabs.iter().any(|item| item.id == tab) => {
                Some(value.facts.settled_at.known().cloned())
            }
            _ => None,
        })
        .flatten();
    assert_eq!(settlement(&snapshot, tab), expected);
    connection.reconnect().unwrap();
    assert!(!connection.is_current(&origin));
    assert!(admission.check().is_err());
    let before = wire.0.lock().unwrap().len();
    let mut stale_applied = false;
    assert!(
        admission
            .commit_if_current(|| stale_applied = true)
            .is_err()
    );
    assert!(!stale_applied);
    assert!(
        connection
            .execute_admitted(
                &admission,
                &m::GetHomeDirectory(shellfox_protocol::GetHomeDirectory {})
            )
            .await
            .is_err()
    );
    assert_eq!(wire.0.lock().unwrap().len(), before);
    let fresh = connection.connect().await.unwrap();
    assert!(!fresh.witness.same_origin(&origin.witness));
    connection.disconnect().await.unwrap();
    assert_eq!(connection.status(), Status::Disconnected);
}
