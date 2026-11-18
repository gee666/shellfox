use crate::{backend::PeerDocumentBackend, connection::PeerConnection};
use serde_json::{Value, json};
use shellfox_client::manager_v2::ManagerClient;
use shellfox_files::EditorCancellation;
use shellfox_protocol::{self as p, manager_v2 as m, *};
use std::sync::{Arc, Mutex, Weak};
pub const PATH: &str = "/peer/project/document.txt";
#[derive(Default)]
pub struct State {
    pub body: Vec<u8>,
    pub stage: Vec<u8>,
    pub revision: String,
    pub calls: Vec<String>,
    pub bindings: Vec<Value>,
    pub expected_revision: Option<String>,
    pub read_other_target: bool,
    pub finish_other_target: bool,
    pub lose_finish: bool,
    pub cancel_on_write: Option<EditorCancellation>,
    pub retire_on_finish: Option<Weak<ManagerClient<Fake>>>,
}
#[derive(Clone, Default)]
pub struct Fake(pub Arc<Mutex<State>>);
fn reply(value: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"version":2,"ok":true,"value":value})).unwrap()
}
impl OutboundTransport for Fake {
    fn send<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        _: RequestLimits,
    ) -> TransportFuture<'a> {
        assert_eq!(peer.as_str(), "peer-under-test");
        Box::pin(async move {
            if endpoint == Endpoint::DiscoverV2 {
                return Ok(std::fs::read(format!(
                    "{}/../shellfox-protocol/tests/fixtures/manager_v2/discovery.json",
                    env!("CARGO_MANIFEST_DIR")
                ))
                .unwrap());
            }
            if endpoint == Endpoint::PairV2 {
                let offer: m::PairOffer = serde_json::from_slice(&body).unwrap();
                self.0.lock().unwrap().calls.push("PAIR".into());
                return Ok(reply(
                    serde_json::to_value(m::PairSelection {
                        client_id: offer.client_id,
                        attempt_id: offer.attempt_id,
                        owner_id: offer.owner_id,
                        connection_id: m::Id::new(),
                        profile: offer.profile,
                        catalog_mode: offer.catalog_mode,
                        delivery_format: offer.delivery_format,
                        recovery_format: offer.recovery_format,
                        input_format: offer.input_format,
                        device_policy: m::DevicePolicy::ControlFullAccount,
                        device_permission: m::DevicePermission::Write,
                        native0_sender: None,
                        native0_receiver: None,
                    })
                    .unwrap(),
                ));
            }
            assert_eq!(endpoint, Endpoint::RequestV2);
            let request: Value = serde_json::from_slice(&body).unwrap();
            let method = request["method"].as_str().unwrap().to_owned();
            let payload = &request["payload"];
            let mut state = self.0.lock().unwrap();
            state.calls.push(method.clone());
            state.bindings.push(request["binding"].clone());
            let value = match method.as_str() {
                "getHomeDirectory" => json!({"cwd":"/peer/project"}),
                "listFiles" => {
                    json!({"path":"/peer/project","parentPath":"/peer","entries":[{"name":"document.txt","path":PATH,"kind":"file"}],"truncated":false})
                }
                "beginFileRead" => {
                    assert_eq!(payload["path"], PATH);
                    json!({"transferId":m::Id::new(),"path":if state.read_other_target{"/peer/other.txt"}else{PATH},"size":state.body.len(),"revision":state.revision,"kind":"text","mimeType":"text/plain","chunkBytes":p::FILE_CHUNK_BYTES})
                }
                "readFileChunk" => {
                    let offset = payload["offset"].as_u64().unwrap() as usize;
                    let end = (offset + p::FILE_CHUNK_BYTES).min(state.body.len());
                    json!({"offset":end,"data":p::encode_chunk(&state.body[offset..end]).unwrap(),"eof":end==state.body.len()})
                }
                "beginFileWrite" => {
                    assert_eq!(payload["path"], PATH);
                    assert_eq!(payload["revision"], state.revision);
                    assert_eq!(payload["mode"], "replace");
                    assert_eq!(payload["content"], "text");
                    state.expected_revision = payload["revision"].as_str().map(str::to_owned);
                    state.stage.clear();
                    json!({"transferId":m::Id::new(),"chunkBytes":p::FILE_CHUNK_BYTES})
                }
                "writeFileChunk" => {
                    assert_eq!(
                        payload["offset"].as_u64().unwrap(),
                        state.stage.len() as u64
                    );
                    let bytes = p::decode_chunk(payload["data"].as_str().unwrap()).unwrap();
                    state.stage.extend(bytes);
                    if let Some(cancel) = &state.cancel_on_write {
                        cancel.cancel();
                    }
                    json!({"offset":state.stage.len()})
                }
                "finishFileWrite" => {
                    state.body = state.stage.clone();
                    state.revision = "b".repeat(64);
                    if let Some(client) = state.retire_on_finish.take().and_then(|w| w.upgrade()) {
                        client.reconnect(m::Id::new());
                    }
                    if state.lose_finish {
                        return Err(TransportError::Deadline);
                    }
                    json!({"path":if state.finish_other_target{"/peer/other.txt"}else{PATH},"size":state.body.len(),"revision":state.revision})
                }
                "cancelFileTransfer" => json!({"cancelled":true}),
                other => {
                    panic!("No unsupported, fallback, retry or automatic pairing expected: {other}")
                }
            };
            Ok(serde_json::to_vec(
                &json!({"version":2,"binding":request["binding"],"ok":true,"value":value}),
            )
            .unwrap())
        })
    }
}
pub struct Fixture {
    pub fake: Fake,
    pub client: Arc<ManagerClient<Fake>>,
    pub connection: Arc<PeerConnection<Fake>>,
    pub backend: Arc<PeerDocumentBackend<Fake>>,
}
impl Fixture {
    pub async fn new(bytes: Vec<u8>) -> Self {
        let fake = Fake::default();
        {
            let mut s = fake.0.lock().unwrap();
            s.body = bytes;
            s.revision = "a".repeat(64);
        }
        let id = m::Id::new();
        let client = Arc::new(ManagerClient::new(
            AuthenticatedPeerId::new("peer-under-test").unwrap(),
            id,
            fake.clone(),
        ));
        let connection = Arc::new(PeerConnection::new(client.clone(), id));
        let origin = connection.connect().await.unwrap();
        let admission = connection.admit_source_with(&origin.witness).unwrap();
        let backend = PeerDocumentBackend::new(connection.clone(), admission).unwrap();
        Self {
            fake,
            client,
            connection,
            backend,
        }
    }
    pub fn count(&self, method: &str) -> usize {
        self.fake
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|v| v.as_str() == method)
            .count()
    }
}
pub fn run(f: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}
