//! Remote attach against a scripted v1 peer: replay, streamed output with
//! acknowledgements, Ghostty-encoded keys, resize, take-over and detach.
use super::*;
use crossterm::event::{KeyEvent as Crossterm, KeyEventState};
use serde_json::{json, Value};
use shellfox_protocol::{AuthenticatedPeerId, ClientId, Endpoint, RequestLimits, TransportError, TransportFuture};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

const TAB: &str = "22345678-1234-4234-8234-123456789abc";
const SESSION: &str = "12345678-1234-4234-8234-123456789abc";
const GENERATION: &str = "32345678-1234-4234-8234-123456789abc";

#[derive(Default)]
struct Script {
    /// Replies to attachTerminal, in order.
    attachments: VecDeque<Value>,
    /// Event batches returned by successive polls; then empty polls.
    polls: VecDeque<Vec<Value>>,
    /// Method and payload of every request.
    seen: Vec<(String, Value)>,
}
#[derive(Clone, Default)]
struct Peer(Arc<Mutex<Script>>);
impl OutboundTransport for Peer {
    fn send<'a>(&'a self, _: &'a AuthenticatedPeerId, endpoint: Endpoint, body: Vec<u8>, _: RequestLimits) -> TransportFuture<'a> {
        Box::pin(async move {
            let body: Value = serde_json::from_slice(&body).map_err(|_| TransportError::InvalidRequest)?;
            let method = body["method"].as_str().unwrap_or_default().to_owned();
            let ok = |value: Value| Ok(serde_json::to_vec(&json!({ "ok": true, "value": value })).unwrap());
            let batch = {
                let mut script = self.0.lock().unwrap();
                match endpoint {
                    Endpoint::Pair => return ok(json!({ "paired": true, "terminalControl": "exclusive-v1" })),
                    Endpoint::Events => script.polls.pop_front(),
                    _ => {
                        script.seen.push((method.clone(), body["payload"].clone()));
                        return match method.as_str() {
                            "attachTerminal" => ok(script.attachments.pop_front().unwrap()),
                            "takeTerminalControl" => ok(json!({ "taken": true })),
                            "writeTerminal" => ok(json!({ "written": true })),
                            "resizeTerminal" => ok(json!({ "resized": true })),
                            "acknowledgeTerminal" => ok(json!({ "acknowledged": true })),
                            "detachTerminal" => ok(json!({ "detached": true })),
                            _ => Err(TransportError::InvalidRequest),
                        };
                    }
                }
            };
            if batch.is_none() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let events: Vec<Value> = batch.unwrap_or_default().into_iter().map(|event| json!({ "type": "terminal", "event": event })).collect();
            ok(json!({ "events": events }))
        })
    }
}

fn owned(epoch: u64, last: u64, text: &str) -> Value {
    json!({
        "control": "owned", "tabId": TAB, "sessionId": SESSION, "generation": GENERATION,
        "controlEpoch": epoch, "firstSequence": 1, "lastSequence": last,
        "chunks": [{ "type": "data", "tabId": TAB, "generation": GENERATION, "sequence": last, "data": text }],
        "truncated": false, "state": "open", "exitCode": null, "cols": 80, "rows": 24, "lifetime": "app-owned",
    })
}
fn data(epoch: u64, sequence: u64, text: &str) -> Value {
    json!({ "type": "data", "tabId": TAB, "generation": GENERATION, "controlEpoch": epoch, "sequence": sequence, "data": text })
}
fn key(code: KeyCode, modifiers: KeyModifiers) -> std::io::Result<Event> {
    Ok(Event::Key(Crossterm { code, modifiers, kind: KeyEventKind::Press, state: KeyEventState::NONE }))
}
/// Scripted keyboard: each event after a pause, so the output stream runs between them.
fn typed(events: Vec<std::io::Result<Event>>) -> impl Stream<Item = std::io::Result<Event>> + Unpin {
    Box::pin(futures_util::stream::iter(events).then(|event| async {
        tokio::time::sleep(Duration::from_millis(150)).await;
        event
    }))
}
async fn paired(peer: &Peer) -> Client<Peer> {
    let client = Client::new(AuthenticatedPeerId::new("n-box".to_owned()).unwrap(), ClientId::new_v4(), peer.clone());
    client.pair().await.unwrap();
    client
}
fn methods(peer: &Peer) -> Vec<String> {
    peer.0.lock().unwrap().seen.iter().map(|(m, _)| m.clone()).collect()
}

#[tokio::test]
async fn output_streams_keys_are_encoded_and_detach_leaves_the_shell() {
    let peer = Peer::default();
    {
        let mut script = peer.0.lock().unwrap();
        script.attachments.push_back(owned(2, 1, "hello"));
        script.polls.push_back(vec![data(2, 2, " world")]);
    }
    let client = paired(&peer).await;
    let mut output = Vec::new();
    let input = typed(vec![
        key(KeyCode::Char('a'), KeyModifiers::NONE),
        key(KeyCode::Up, KeyModifiers::NONE),
        Ok(Event::Resize(100, 30)),
        key(KeyCode::Char(']'), KeyModifiers::CONTROL),
    ]);
    run(&client, TAB.parse().unwrap(), (90, 40), &mut output, input).await.unwrap();
    let screen = String::from_utf8_lossy(&output);
    assert!(screen.contains("hello") && screen.contains(" world"), "{screen}");
    let seen = peer.0.lock().unwrap().seen.clone();
    let writes: Vec<&str> = seen.iter().filter(|(m, _)| m == "writeTerminal").map(|(_, p)| p["data"].as_str().unwrap()).collect();
    assert_eq!(writes, ["a", "\x1b[A"]);
    let acks: Vec<u64> = seen.iter().filter(|(m, _)| m == "acknowledgeTerminal").map(|(_, p)| p["sequence"].as_u64().unwrap()).collect();
    assert_eq!(acks, [1, 2]);
    let sizes: Vec<(u64, u64)> = seen
        .iter()
        .filter(|(m, _)| m == "resizeTerminal")
        .map(|(_, p)| (p["cols"].as_u64().unwrap(), p["rows"].as_u64().unwrap()))
        .collect();
    assert_eq!(sizes, [(90, 40), (100, 30)]);
    assert_eq!(seen.last().unwrap().0, "detachTerminal");
    assert!(!methods(&peer).iter().any(|m| m == "closeTab"));
}

#[tokio::test]
async fn a_held_terminal_is_taken_only_with_ctrl_t_and_exit_ends_attach() {
    let peer = Peer::default();
    {
        let mut script = peer.0.lock().unwrap();
        script.attachments.push_back(json!({
            "control": "blocked", "tabId": TAB, "sessionId": SESSION, "generation": GENERATION, "controlEpoch": 2,
        }));
        script.attachments.push_back(owned(3, 4, "mine"));
    }
    let client = paired(&peer).await;
    let mut output = Vec::new();
    let exit = json!({
        "type": "exit", "tabId": TAB, "generation": GENERATION, "controlEpoch": 3,
        "exitCode": 0, "signal": null, "lastSequence": 4,
    });
    let pushed = peer.clone();
    let input = typed(vec![
        // Typing while blocked sends nothing.
        key(KeyCode::Char('x'), KeyModifiers::NONE),
        key(KeyCode::Char('t'), KeyModifiers::CONTROL),
    ])
    .chain(futures_util::stream::once(async move {
        pushed.0.lock().unwrap().polls.push_back(vec![exit]);
        std::future::pending::<std::io::Result<Event>>().await
    }));
    let input = Box::pin(input);
    tokio::time::timeout(Duration::from_secs(10), run(&client, TAB.parse().unwrap(), (80, 24), &mut output, input))
        .await
        .expect("exit ends attach")
        .unwrap();
    let screen = String::from_utf8_lossy(&output);
    assert!(screen.contains("Another client is controlling this terminal.") && screen.contains("mine"), "{screen}");
    assert_eq!(
        methods(&peer),
        ["attachTerminal", "takeTerminalControl", "attachTerminal", "acknowledgeTerminal", "resizeTerminal", "detachTerminal"]
    );
}
