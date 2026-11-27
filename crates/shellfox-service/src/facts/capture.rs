//! One mapper for immutable evidence from the current registered Runtime.
#[cfg(test)]
mod tests;
use crate::{
    activity::integration::{SourceFacts, StartupEvidence, TerminalSourceFacts},
    terminals::OwnedTerminal,
    Service,
};
pub(crate) fn terminal(service: &Service, terminal: &OwnedTerminal) -> TerminalSourceFacts {
    let mut facts = service
        .activity
        .owner
        .terminal_facts(terminal.runtime.identity(), terminal.runtime.phase());
    facts.source_kind = Some(terminal.runtime.source_kind());
    facts.automation_exit_code = service.remote_automation_exit_code(terminal);
    facts.startup = terminal
        .startup
        .as_ref()
        .map(StartupEvidence::from_started)
        .unwrap_or_default();
    if let StartupEvidence::Failed { acknowledged, .. } = &mut facts.startup {
        *acknowledged = terminal.startup_error_acknowledged;
    }
    facts.title = super::title::published(&terminal.runtime);
    facts
}
/// Whether `source` still shows the current metadata revision and every
/// member terminal's identity, phase, startup result and published title.
/// Clients read a tab's lifecycle and title from these facts, so a change
/// here can't wait for the next poll.
pub(crate) fn lifecycle_current(service: &Service, source: &SourceFacts) -> bool {
    fn startup(evidence: &StartupEvidence) -> u8 {
        match evidence {
            StartupEvidence::Pending => 0,
            StartupEvidence::Confirmed => 1,
            StartupEvidence::Failed { .. } => 2,
        }
    }
    let mut current: Vec<_> = service
        .terminals
        .values()
        .filter(|terminal| {
            crate::facts::membership::contains(&source.snapshot, terminal.runtime.identity())
        })
        .map(|terminal| {
            let started = match &terminal.startup {
                None => 0,
                Some(Ok(())) => 1,
                Some(Err(_)) => 2,
            };
            (
                terminal.runtime.identity(),
                terminal.runtime.phase(),
                started,
                super::title::published(&terminal.runtime),
            )
        })
        .collect();
    current.sort_by_key(|(identity, ..)| identity.tab_id);
    source.snapshot.revision == service.host.revision()
        && current.len() == source.terminals.len()
        && current
            .iter()
            .zip(&source.terminals)
            .all(|(now, published)| {
                (now.0, now.1, now.2, now.3.as_deref())
                    == (
                        published.identity,
                        published.phase,
                        startup(&published.startup),
                        published.title.as_deref(),
                    )
            })
}
