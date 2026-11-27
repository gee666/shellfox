//! Agent status for a WSL tab from guest processes that carry its marker, as
//! 0.9 counted them. The Windows Job owns only wsl.exe, so this evidence is
//! status only: never root, descendant or ownership evidence, and nothing is
//! signalled because of it.
use super::matching;
use shellfox_core::SettingsProcessRule;
use shellfox_terminal::wsl::GuestProcesses;
use std::sync::Arc;

pub enum GuestEvidence {
    /// No current guest listing: health is unknown, never idle.
    Unavailable,
    /// One distribution's listing, shared by its tabs, and this tab's marker.
    Listed {
        listing: Arc<GuestProcesses>,
        marker: String,
    },
}

/// Adds the tab's matching guest processes to `agents`. Anything that keeps
/// the count from being exact sets `reason`, so health becomes unknown.
pub(super) fn count(
    evidence: &GuestEvidence,
    rules: &[SettingsProcessRule],
    agents: &mut u32,
    reason: &mut Option<&'static str>,
) {
    let GuestEvidence::Listed { listing, marker } = evidence else {
        *reason = Some("Guest process inspection is unavailable.");
        return;
    };
    if !listing.complete {
        *reason = Some("Marked guest process enumeration was incomplete.");
    }
    for process in listing.processes.iter().filter(|p| p.marker == *marker) {
        let Some(executable) = process.executable.as_deref().filter(|_| process.accessible) else {
            *reason = Some("A marked guest process is inaccessible.");
            continue;
        };
        let (matched, failure) = matching::match_agent(
            executable,
            process.process_name.as_deref(),
            process.argv.as_deref(),
            rules,
            false,
        );
        if failure.is_some() {
            *reason = failure;
        }
        *agents += u32::from(matched);
    }
}

#[cfg(test)]
mod tests;
