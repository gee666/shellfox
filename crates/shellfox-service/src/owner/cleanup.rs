use crate::{ActivityShutdownState, Service, ShutdownFailure, ShutdownOutcome};
use std::time::Duration;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod warning_tests;

pub(super) fn committed() -> ShutdownOutcome {
    ShutdownOutcome {
        activity: ActivityShutdownState::Committed,
        presentation: crate::PresentationShutdownState::NoKnownFailure,
    }
}
/// Retries cleanup until it is confirmed. It never gives up and exits: an
/// exit would drop terminals whose close isn't confirmed. Each 30 s without
/// confirmation is logged.
pub(super) fn finish(service: Service) -> ShutdownOutcome {
    const LOG_EVERY: usize = 300;
    let mut attempts = 0;
    finish_with(service, |_, error| {
        attempts += 1;
        if attempts % LOG_EVERY == 0 {
            eprintln!(
                "Shellfox host: service cleanup not confirmed after {} s: {error}. Still retrying.",
                attempts / 10
            );
        }
        std::thread::sleep(Duration::from_millis(100))
    })
}
fn finish_with(
    mut service: Service,
    mut retained: impl FnMut(&mut Service, &ShutdownFailure),
) -> ShutdownOutcome {
    let mut previous = None;
    loop {
        match service.shutdown() {
            Ok(outcome) => {
                drop(service);
                return outcome;
            }
            Err(error) => {
                let resource_failure = matches!(error, ShutdownFailure::ResourceCleanup(_));
                if previous != Some(resource_failure) {
                    eprintln!("{}", error); // Distinguishes stopped processes from pending metadata.
                    previous = Some(resource_failure);
                }
                retained(&mut service, &error);
            }
        }
    }
}
#[derive(Debug)]
pub(crate) struct StartupFailure {
    pub cause: anyhow::Error,
    pub shutdown: ShutdownOutcome,
}
impl std::fmt::Display for StartupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.cause, f)
    }
}
impl std::error::Error for StartupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}
pub(crate) fn report_warning(outcome: &ShutdownOutcome) {
    if let crate::PresentationShutdownState::FailedSources(warnings) = &outcome.presentation {
        eprintln!("Warning: {} automation source presentations did not complete. Original raw cleanup and parser-thread joins are confirmed; opaque source evidence is retained.", warnings.failures().len());
    }
    if let ActivityShutdownState::DirtyAfterOwnershipLoss { pending_sessions } = outcome.activity {
        eprintln!("Warning: {pending_sessions} session activity updates were not saved after ownership loss. Owned resources are confirmed closed.");
    }
}
