use futures::executor::block_on;
use shellfox_client::{protocol::*, *};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

const PAIR: &[u8] = include_bytes!("fixtures/remote-pair-success.json");
const SNAPSHOT: &[u8] = include_bytes!("fixtures/manager-snapshot-success.json");

#[derive(Default)]
struct Replies(Mutex<Vec<Vec<u8>>>);
impl OutboundTransport for Replies {
    fn send<'a>(
        &'a self,
        _: &'a AuthenticatedPeerId,
        _: Endpoint,
        _: Vec<u8>,
        _: RequestLimits,
    ) -> TransportFuture<'a> {
        let reply = self.0.lock().unwrap().remove(0);
        Box::pin(async move { Ok(reply) })
    }
}

fn client(replies: &[&[u8]]) -> Client<Replies> {
    let transport = Replies(Mutex::new(replies.iter().map(|r| r.to_vec()).collect()));
    Client::new(
        AuthenticatedPeerId::new("host-1").unwrap(),
        "10000000-0000-4000-8000-000000000001".parse().unwrap(),
        transport,
    )
}
fn counter(client: &Client<Replies>) -> Arc<AtomicUsize> {
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = wakes.clone();
    client.set_state_waker(Some(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })));
    wakes
}

#[test]
fn wakes_on_connection_changes_but_not_on_ordinary_requests() {
    let client = client(&[PAIR, SNAPSHOT]);
    let wakes = counter(&client);
    block_on(client.pair()).unwrap();
    // Disconnected -> Connecting, then Connecting -> Connected with a new id.
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    block_on(client.execute(&GetSnapshot {})).unwrap();
    assert_eq!(wakes.load(Ordering::SeqCst), 2, "a successful request changes nothing");
    client.suspend();
    assert_eq!(wakes.load(Ordering::SeqCst), 3);
    client.suspend();
    assert_eq!(wakes.load(Ordering::SeqCst), 3, "no change, no wake");
}

#[test]
fn a_cleared_waker_is_never_called() {
    let client = client(&[PAIR]);
    let wakes = counter(&client);
    client.set_state_waker(None);
    block_on(client.pair()).unwrap();
    assert_eq!(wakes.load(Ordering::SeqCst), 0);
}

#[test]
fn the_waker_may_read_client_state() {
    let client = Arc::new(client(&[PAIR]));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (reader, log) = (Arc::downgrade(&client), seen.clone());
    client.set_state_waker(Some(Arc::new(move || {
        if let Some(client) = reader.upgrade() {
            log.lock().unwrap().push(client.state().status);
        }
    })));
    block_on(client.pair()).unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [ConnectionStatus::Connecting, ConnectionStatus::Connected]
    );
}
