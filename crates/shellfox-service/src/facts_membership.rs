//! Runtime descriptors describe catalog members only, never hidden reservations.
use shellfox_core::Snapshot;
use shellfox_runtime::Identity;
pub(crate) fn contains(snapshot: &Snapshot, identity: Identity) -> bool {
    snapshot.sessions.iter().any(|session| {
        session.id == identity.session_id
            && session
                .tabs
                .iter()
                .any(|tab| tab.id == identity.tab_id && tab.session_id == identity.session_id)
    })
}
