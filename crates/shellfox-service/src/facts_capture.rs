//! One mapper for immutable evidence from the current registered Runtime.
use crate::{
    activity_integration::{OwnerActivity, StartupEvidence, TerminalSourceFacts},
    terminals::OwnedTerminal,
};
pub(crate) fn terminal(owner: &OwnerActivity, terminal: &OwnedTerminal) -> TerminalSourceFacts {
    let mut facts = owner.terminal_facts(terminal.runtime.identity(), terminal.runtime.phase());
    facts.source_kind = Some(terminal.runtime.source_kind());
    facts.startup = terminal
        .startup
        .as_ref()
        .map(StartupEvidence::from_started)
        .unwrap_or_default();
    if let StartupEvidence::Failed { acknowledged, .. } = &mut facts.startup {
        *acknowledged = terminal.startup_error_acknowledged;
    }
    facts
}
