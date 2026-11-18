//! Opt-in observer-v2 backend components. No HTTP route is enabled here.
#[path = "remote_v2_delivery.rs"]
pub mod delivery;
#[path = "remote_v2_mailboxes.rs"]
pub mod mailboxes;
#[cfg(test)]
#[path = "remote_v2_owner_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "remote_v2_retirement_tests.rs"]
mod retirement_tests;

/// Descriptive source evidence only. Profile strings, creation intent and phase
/// cannot substitute for the kind captured from this exact registered Runtime.
pub fn source_kind(
    facts: Option<&crate::activity_integration::TerminalSourceFacts>,
) -> shellfox_protocol::observer_v2::Fact<shellfox_protocol::observer_v2::SourceKind> {
    use shellfox_protocol::observer_v2::{Fact, SourceKind, UnknownReason};
    match facts.and_then(|facts| facts.source_kind) {
        Some(shellfox_runtime::SourceKind::OwnedPty) => Fact::Known {
            value: SourceKind::Pty,
        },
        Some(shellfox_runtime::SourceKind::ExternalBytes) => Fact::Known {
            value: SourceKind::ExternalBytes,
        },
        None => Fact::Unknown {
            reason: UnknownReason::Unavailable,
        },
    }
}
