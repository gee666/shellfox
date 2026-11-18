//! Only the producer's closed, non-sensitive health messages may reach clients.
pub(crate) fn message(value: &str) -> &'static str {
    match value {
        "Cannot read embedded Tailscale connectivity. Retrying the health check." => {
            "Cannot read embedded Tailscale connectivity. Retrying the health check."
        }
        "Embedded Tailscale is stopped." => "Embedded Tailscale is stopped.",
        "Embedded Tailscale is unavailable. Retrying the health check." => {
            "Embedded Tailscale is unavailable. Retrying the health check."
        }
        "Network connection unavailable. Tailscale will reconnect automatically." => {
            "Network connection unavailable. Tailscale will reconnect automatically."
        }
        "Tailscale control connection unavailable. Reconnecting automatically." => {
            "Tailscale control connection unavailable. Reconnecting automatically."
        }
        "Tailscale relay or socket connectivity is degraded. Repair is retrying automatically." => {
            "Tailscale relay or socket connectivity is degraded. Repair is retrying automatically."
        }
        "Tailscale reports degraded connectivity." => "Tailscale reports degraded connectivity.",
        _ => "Tailscale network unavailable.",
    }
}
