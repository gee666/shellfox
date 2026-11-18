//! Volatile live-process metadata, not a saved user title or file permission.
use crate::{Identity, RuntimeError};
use shellfox_ghostty_vt::{Terminal, TerminalMetadata};
use std::sync::{Arc, Mutex};

/// Deliberately not Debug/Serialize: application titles can contain command text.
/// Counters are cumulative for this runtime generation; reads never drain them.
pub struct RuntimeMetadata {
    pub identity: Identity,
    pub sequence: u64,
    pub version: u64,
    pub title: Option<String>,
    /// Untrusted URI from Ghostty's public PWD callback, not OSC-7 provenance.
    pub cwd_uri: Option<String>,
    pub bell_count: u64,
    pub bells_saturated: bool,
    pub rejected_updates: u64,
}
pub(crate) fn collect(
    terminal: &mut Terminal,
    slot: &Mutex<Option<Arc<RuntimeMetadata>>>,
    identity: Identity,
    sequence: u64,
) -> Result<(), RuntimeError> {
    if let Some(update) = terminal
        .take_metadata()
        .map_err(RuntimeError::unavailable)?
    {
        let mut current = slot.lock().unwrap_or_else(|error| error.into_inner());
        let next = merge(current.as_deref(), update, identity, sequence);
        *current = Some(Arc::new(next));
    }
    Ok(())
}
fn merge(
    prior: Option<&RuntimeMetadata>,
    update: TerminalMetadata,
    identity: Identity,
    sequence: u64,
) -> RuntimeMetadata {
    let bells = prior.map_or(0, |prior| prior.bell_count);
    let next_bells = bells.checked_add(u64::from(update.bell_count));
    RuntimeMetadata {
        identity,
        sequence,
        version: update.version,
        title: update.title,
        cwd_uri: update.cwd_uri,
        bell_count: next_bells.unwrap_or(u64::MAX),
        bells_saturated: update.bells_saturated
            || next_bells.is_none()
            || prior.is_some_and(|prior| prior.bells_saturated),
        rejected_updates: prior
            .map_or(0, |prior| prior.rejected_updates)
            .saturating_add(u64::from(update.rejected_updates)),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use shellfox_core::{Generation, SessionId, TabId};
    #[test]
    fn coalesced_metadata_keeps_unseen_bells_and_rejections_without_replaying_them() {
        let identity = Identity {
            session_id: SessionId::new(),
            tab_id: TabId::new(),
            generation: Generation::new(),
        };
        let mut terminal = Terminal::new(12, 3).unwrap();
        let slot = Mutex::new(None);
        terminal
            .feed(b"\x1b]2;first\x07\x07\x1b]7;file://foreign.example/project\x07")
            .unwrap();
        collect(&mut terminal, &slot, identity, 1).unwrap();
        let initial = Arc::clone(slot.lock().unwrap().as_ref().unwrap());
        assert_eq!(initial.title.as_deref(), Some("first"));
        assert_eq!(initial.bell_count, 1);
        assert_eq!(
            initial.cwd_uri.as_deref(),
            Some("file://foreign.example/project")
        );
        terminal.feed(b"\x1b]2;second\x07").unwrap();
        collect(&mut terminal, &slot, identity, 2).unwrap();
        let latest = Arc::clone(slot.lock().unwrap().as_ref().unwrap());
        assert!(latest.version > initial.version);
        assert_eq!(latest.bell_count, 1);
        assert_eq!(latest.title.as_deref(), Some("second"));
        terminal
            .feed(b"\x1b]7;file://user:password@foreign.example/project\x07")
            .unwrap();
        collect(&mut terminal, &slot, identity, 3).unwrap();
        let rejected = Arc::clone(slot.lock().unwrap().as_ref().unwrap());
        assert!(rejected.rejected_updates > latest.rejected_updates);
        assert_eq!(rejected.cwd_uri, latest.cwd_uri);
        terminal.render().unwrap();
        terminal.format_vt(0xfff, 1024 * 1024).unwrap();
        collect(&mut terminal, &slot, identity, 2).unwrap();
        assert!(Arc::ptr_eq(
            &rejected,
            slot.lock().unwrap().as_ref().unwrap()
        ));
        assert_eq!(initial.title.as_deref(), Some("first"));
    }
}
