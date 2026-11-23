//! Only the producer's closed, non-sensitive health messages may reach clients.
const KNOWN: [&str; 7] = [
    "Cannot read embedded Tailscale connectivity. Retrying the health check.",
    "Embedded Tailscale is stopped.",
    "Embedded Tailscale is unavailable. Retrying the health check.",
    "Network connection unavailable. Tailscale will reconnect automatically.",
    "Tailscale control connection unavailable. Reconnecting automatically.",
    "Tailscale relay or socket connectivity is degraded. Repair is retrying automatically.",
    "Tailscale reports degraded connectivity.",
];

pub(crate) fn message(value: &str) -> &'static str {
    KNOWN
        .into_iter()
        .find(|known| *known == value)
        .unwrap_or("Tailscale network unavailable.")
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_closed_health_messages_escape_unknown_provider_errors() {
        for known in super::KNOWN {
            assert_eq!(super::message(known), known);
        }
        for unknown in [
            "upstream secret tskey-auth-DO-NOT-ECHO",
            "https://login.example/secret",
            "private source failure",
        ] {
            assert_eq!(super::message(unknown), "Tailscale network unavailable.");
        }
    }
}
