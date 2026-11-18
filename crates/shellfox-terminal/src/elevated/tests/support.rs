use super::super::*;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    io,
    path::PathBuf,
    rc::Rc,
};

#[derive(Default)]
pub struct State {
    pub sent: Vec<Vec<u8>>,
    pub incoming: VecDeque<HelperFrame>,
    pub disconnected: bool,
    pub fail_send: bool,
    pub would_block: bool,
    pub backlog: usize,
}
#[derive(Clone, Default)]
pub struct Fake(pub Rc<RefCell<State>>);
impl HelperIo for Fake {
    fn send_line(&mut self, line: &[u8]) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        if state.would_block {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        state.sent.push(line.to_vec());
        if state.fail_send {
            return Err(io::Error::other("synthetic transport failure"));
        }
        Ok(())
    }
    fn try_receive_line(&mut self) -> io::Result<Option<HelperFrame>> {
        Ok(self.0.borrow_mut().incoming.pop_front())
    }
    fn queued_write_bytes(&self) -> usize {
        self.0.borrow().backlog
    }
    fn disconnect_input(&mut self) {
        self.0.borrow_mut().disconnected = true;
    }
}
pub fn generation() -> Generation {
    Generation::new(7).unwrap()
}
pub fn launch() -> PreparedLaunch {
    Launch::fixture(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        BTreeMap::new(),
        80,
        24,
    )
    .unwrap()
    .prepare()
    .unwrap()
}
pub fn start() -> (Channel<Fake>, Fake) {
    let io = Fake::default();
    let channel = Channel::start(io.clone(), generation(), launch()).unwrap();
    (channel, io)
}
pub fn active() -> (Channel<Fake>, Fake) {
    let (mut channel, io) = start();
    send(
        &io,
        json!({"type":"ready", "pid":456, "birth":"133000000000000000"}),
    );
    assert!(matches!(
        channel.poll(generation()).unwrap(),
        Some(Event::Ready(_))
    ));
    (channel, io)
}
pub fn send(io: &Fake, value: Value) {
    raw(io, serde_json::to_vec(&value).unwrap());
}
pub fn raw(io: &Fake, bytes: Vec<u8>) {
    io.0.borrow_mut()
        .incoming
        .push_back(HelperFrame::Line(bytes));
}
pub fn eof(io: &Fake) {
    io.0.borrow_mut().incoming.push_back(HelperFrame::Eof);
}
pub fn sent(io: &Fake) -> Vec<Value> {
    io.0.borrow()
        .sent
        .iter()
        .map(|bytes| serde_json::from_slice(bytes).unwrap())
        .collect()
}
