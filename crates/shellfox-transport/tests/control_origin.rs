#![cfg(feature = "helper")]

use shellfox_transport::LoopbackOrigin;
#[test]
fn control_origin_is_explicit_literal_loopback_and_bounded() {
    for value in [
        "http://127.0.0.1:1234",
        "http://127.0.0.1:1234/",
        "http://[::1]:1234",
    ] {
        assert!(LoopbackOrigin::parse_control_url(value).is_ok());
    }
    for value in [
        "https://127.0.0.1:1234",
        "http://localhost:1234",
        "http://192.0.2.1:1234",
        "http://127.0.0.1:0",
        "http://127.0.0.1:65536",
        "http://127.0.0.1",
        "http://user:password@127.0.0.1:1234",
        "http://127.0.0.1:1234/path",
        "http://127.0.0.1:1234?",
        "http://127.0.0.1:1234#",
        "http://[::ffff:127.0.0.1]:1234",
        "http://[::1%25lo]:1234",
        "relative",
    ] {
        assert!(LoopbackOrigin::parse_control_url(value).is_err());
    }
    assert!(LoopbackOrigin::parse_control_url(&"a".repeat(129)).is_err());
}
