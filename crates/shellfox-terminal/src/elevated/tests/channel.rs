use super::super::*;
use super::support::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;

#[test]
fn private_launch_precedes_ready_and_utf8_remains_raw_until_terminal_parser() {
    let (mut channel, io) = start();
    assert_eq!(sent(&io)[0]["type"], "launch-isolated");
    assert!(sent(&io)[0]["isolationRoot"].is_string());
    assert_eq!(channel.phase(), Phase::Starting);
    assert_eq!(
        channel.write(generation(), b"x"),
        Err(AdapterError::InputUnavailable)
    );
    send(
        &io,
        json!({"type":"ready", "pid":456, "birth":"133000000000000000"}),
    );
    assert!(matches!(
        channel.poll(generation()).unwrap(),
        Some(Event::Ready(_))
    ));
    assert_eq!(channel.identity().unwrap().pid.get(), 456);
    assert_eq!(
        channel.identity().unwrap().birth.as_str(),
        "133000000000000000"
    );
    let bytes = "雪🙂\x1b[31m".as_bytes();
    let mut observed = Vec::new();
    for chunk in bytes.chunks(2) {
        send(&io, json!({"type":"data", "data":STANDARD.encode(chunk)}));
        if let Some(Event::Data(bytes)) = channel.poll(generation()).unwrap() {
            observed.extend(bytes);
        }
    }
    assert_eq!(observed, bytes);
}

#[test]
fn exact_ordered_acknowledgements_release_bounded_input_credit() {
    let (mut channel, io) = active();
    assert_eq!(channel.write(generation(), &vec![b'x'; 65536]).unwrap(), 1);
    assert_eq!(channel.write(generation(), &vec![b'y'; 65536]).unwrap(), 2);
    assert_eq!(channel.queued_input_bytes(), Some(MAX_INPUT));
    assert_eq!(
        channel.write(generation(), b"z"),
        Err(AdapterError::QueueFull)
    );
    send(&io, json!({"type":"ack", "id":1}));
    assert_eq!(
        channel.poll(generation()).unwrap(),
        Some(Event::InputAcknowledged(1))
    );
    assert_eq!(channel.write(generation(), b"z").unwrap(), 3);
    for id in [2, 3] {
        send(&io, json!({"type":"ack", "id":id}));
        channel.poll(generation()).unwrap();
    }
    assert_eq!(channel.queued_input_bytes(), Some(0));
    send(&io, json!({"type":"ack", "id":3}));
    assert_eq!(
        channel.poll(generation()),
        Err(AdapterError::InvalidProtocol)
    );
    assert!(!channel.cleanup_confirmed());
    assert!(io.0.borrow().disconnected);
}

#[test]
fn escaped_frame_rejection_does_not_consume_sequence_or_disconnect() {
    let (mut channel, io) = active();
    channel.write(generation(), &vec![b'\''; 50000]).unwrap();
    assert!(io.0.borrow().sent.last().unwrap().len() < MAX_FRAME);
    send(&io, json!({"type":"ack", "id":1}));
    channel.poll(generation()).unwrap();
    let before = sent(&io).len();
    assert_eq!(
        channel.write(generation(), &vec![1; 50000]),
        Err(AdapterError::InvalidInput)
    );
    assert_eq!(channel.queued_input_bytes(), Some(0));
    assert_eq!(sent(&io).len(), before);
    assert!(!io.0.borrow().disconnected);
    assert_eq!(channel.write(generation(), b"still usable").unwrap(), 2);
    channel.resize(generation(), 80, 500).unwrap();
    assert_eq!(
        sent(&io).last().unwrap(),
        &json!({"type":"resize", "cols":80, "rows":500})
    );
    for rows in [1, 501] {
        assert_eq!(
            channel.resize(generation(), 80, rows),
            Err(AdapterError::InvalidInput)
        );
    }
    io.0.borrow_mut().backlog = MAX_FRAME + 1;
    assert_eq!(
        channel.resize(generation(), 80, 24),
        Err(AdapterError::QueueFull)
    );
}

#[test]
fn stale_generation_never_touches_transport_or_cleanup() {
    let (mut channel, io) = active();
    let stale = Generation::new(generation().get() + 1).unwrap();
    let before = sent(&io).len();
    assert_eq!(
        channel.write(stale, b"x"),
        Err(AdapterError::StaleGeneration)
    );
    assert_eq!(
        channel.resize(stale, 80, 24),
        Err(AdapterError::StaleGeneration)
    );
    assert_eq!(
        channel.begin_close(stale),
        Err(AdapterError::StaleGeneration)
    );
    assert_eq!(channel.poll(stale), Err(AdapterError::StaleGeneration));
    assert_eq!(
        channel.startup_timed_out(stale),
        Err(AdapterError::StaleGeneration)
    );
    assert_eq!(
        channel.close_timed_out(stale),
        Err(AdapterError::StaleGeneration)
    );
    assert_eq!(channel.phase(), Phase::Active);
    assert_eq!(sent(&io).len(), before);
    assert!(!channel.cleanup_confirmed());
}

#[test]
fn cancelled_approval_and_late_ready_cannot_enable_input_but_exit_can_confirm_cleanup() {
    let (mut channel, io) = start();
    channel.begin_close(generation()).unwrap();
    channel.begin_close(generation()).unwrap();
    assert_eq!(sent(&io).len(), 2);
    send(&io, json!({"type":"ready", "pid":456, "birth":"123"}));
    assert!(matches!(
        channel.poll(generation()).unwrap(),
        Some(Event::LateReady(_))
    ));
    assert_eq!(
        channel.write(generation(), b"x"),
        Err(AdapterError::InputUnavailable)
    );
    send(
        &io,
        json!({"type":"error", "code":"cancelled", "message":"do not log this synthetic secret"}),
    );
    let failure = channel.poll(generation()).unwrap();
    assert_eq!(failure, Some(Event::Failed(AdapterError::Cancelled)));
    assert!(!format!("{failure:?}").contains("synthetic secret"));
    assert!(!channel.cleanup_confirmed());
    send(&io, json!({"type":"exit", "exitCode":1}));
    assert_eq!(
        channel.poll(generation()).unwrap(),
        Some(Event::ExitConfirmed(1))
    );
    assert!(channel.cleanup_confirmed());
    eof(&io);
    assert_eq!(channel.poll(generation()).unwrap(), None);
}

#[test]
fn launcher_eof_and_timeout_are_not_job_cleanup_evidence() {
    let (mut channel, io) = active();
    send(
        &io,
        json!({"type":"error", "code":"failed", "message":"untrusted details"}),
    );
    channel.poll(generation()).unwrap();
    assert_eq!(channel.phase(), Phase::Unconfirmed);
    assert_eq!(
        channel.write(generation(), b"x"),
        Err(AdapterError::InputUnavailable)
    );
    eof(&io);
    assert_eq!(
        channel.poll(generation()),
        Err(AdapterError::CleanupUnconfirmed)
    );
    assert!(!channel.cleanup_confirmed());

    let (mut channel, io) = start();
    assert_eq!(
        channel.startup_timed_out(generation()),
        Err(AdapterError::Cancelled)
    );
    assert_eq!(sent(&io).last().unwrap(), &json!({"type":"close"}));
    assert_eq!(
        channel.close_timed_out(generation()),
        Err(AdapterError::CleanupUnconfirmed)
    );
    assert!(!io.0.borrow().disconnected);
    send(&io, json!({"type":"exit", "exitCode":1}));
    assert_eq!(
        channel.poll(generation()).unwrap(),
        Some(Event::ExitConfirmed(1))
    );
    assert!(channel.cleanup_confirmed());
}

#[test]
fn refused_transport_backpressure_releases_credit_and_close_can_retry() {
    let (mut channel, io) = active();
    io.0.borrow_mut().would_block = true;
    let sent_before = sent(&io).len();
    assert_eq!(
        channel.write(generation(), b"not accepted"),
        Err(AdapterError::QueueFull)
    );
    assert_eq!(channel.queued_input_bytes(), Some(0));
    assert_eq!(sent(&io).len(), sent_before);
    assert_eq!(
        channel.resize(generation(), 80, 24),
        Err(AdapterError::QueueFull)
    );
    assert_eq!(channel.phase(), Phase::Active);
    io.0.borrow_mut().would_block = false;
    assert_eq!(channel.write(generation(), b"accepted").unwrap(), 1);
    io.0.borrow_mut().would_block = true;
    assert_eq!(
        channel.begin_close(generation()),
        Err(AdapterError::QueueFull)
    );
    assert_eq!(channel.phase(), Phase::Closing);
    io.0.borrow_mut().would_block = false;
    channel.begin_close(generation()).unwrap();
    assert_eq!(sent(&io).last().unwrap(), &json!({"type":"close"}));
}

#[test]
fn input_closed_is_failure_not_exit_and_authenticated_output_can_still_confirm_peer() {
    let (mut channel, io) = active();
    channel.write(generation(), b"pending delivery").unwrap();
    io.0.borrow_mut()
        .incoming
        .push_back(HelperFrame::InputClosed);
    assert_eq!(
        channel.poll(generation()).unwrap(),
        Some(Event::Failed(AdapterError::Transport))
    );
    assert_eq!(channel.phase(), Phase::Unconfirmed);
    assert!(!channel.cleanup_confirmed());
    assert_eq!(
        channel.write(generation(), b"do not replay"),
        Err(AdapterError::InputUnavailable)
    );
    assert_eq!(
        channel.begin_close(generation()),
        Err(AdapterError::CleanupUnconfirmed)
    );
    send(&io, json!({"type":"exit", "exitCode":1}));
    assert_eq!(
        channel.poll(generation()).unwrap(),
        Some(Event::ExitConfirmed(1))
    );
    assert!(channel.cleanup_confirmed());
}

#[test]
fn partial_startup_or_write_failure_retains_controller_and_never_replays() {
    let io = Fake::default();
    io.0.borrow_mut().fail_send = true;
    let failure = Channel::start(io, generation(), launch()).unwrap_err();
    assert_eq!(failure.error, AdapterError::Transport);
    assert_eq!(failure.retained.phase(), Phase::Unconfirmed);
    assert!(!failure.retained.cleanup_confirmed());

    let (mut channel, io) = active();
    io.0.borrow_mut().fail_send = true;
    assert_eq!(
        channel.write(generation(), b"possibly delivered"),
        Err(AdapterError::Transport)
    );
    let before = sent(&io).len();
    assert_eq!(
        channel.write(generation(), b"possibly delivered"),
        Err(AdapterError::InputUnavailable)
    );
    assert_eq!(sent(&io).len(), before);
    assert!(!channel.cleanup_confirmed());
    // An input-pipe failure does not discredit authenticated output. Keep
    // reading eventual cleanup proof instead of guessing from launcher death.
    send(&io, json!({"type":"exit", "exitCode":1}));
    assert_eq!(
        channel.poll(generation()).unwrap(),
        Some(Event::ExitConfirmed(1))
    );
    assert!(channel.cleanup_confirmed());
}
