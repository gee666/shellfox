#[path = "../src/remote_network_health.rs"]
mod health;
#[test]
fn only_closed_health_messages_escape_unknown_provider_errors() {
    assert_eq!(
        health::message("Embedded Tailscale is stopped."),
        "Embedded Tailscale is stopped."
    );
    assert_eq!(
        health::message("Tailscale control connection unavailable. Reconnecting automatically."),
        "Tailscale control connection unavailable. Reconnecting automatically."
    );
    assert_eq!(
        health::message(
            "Tailscale relay or socket connectivity is degraded. Repair is retrying automatically."
        ),
        "Tailscale relay or socket connectivity is degraded. Repair is retrying automatically."
    );
    for unknown in [
        "upstream secret tskey-auth-DO-NOT-ECHO",
        "https://login.example/secret",
        "private source failure",
    ] {
        assert_eq!(health::message(unknown), "Tailscale network unavailable.");
    }
}
