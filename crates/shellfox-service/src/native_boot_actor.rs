use crate::{ActivityShutdownState, Service, ShutdownFailure, ShutdownOutcome};
use std::time::Duration;
#[cfg(test)]
#[path = "native_boot_actor_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "native_boot_warning_tests.rs"]
mod warning_tests;

pub(super) fn committed() -> ShutdownOutcome {
    ShutdownOutcome {
        activity: ActivityShutdownState::Committed,
        presentation: crate::PresentationShutdownState::NoKnownFailure,
    }
}
/// Retries cleanup for up to 30 s. After that the process exits instead of
/// reporting a confirmed shutdown; the OS then closes the lifetime socket, so
/// clients see the host as lost.
pub(super) fn finish(service: Service) -> ShutdownOutcome {
    const ATTEMPTS: usize = 300;
    let mut attempts = 0;
    finish_with(service, |_, error| {
        attempts += 1;
        if attempts >= ATTEMPTS {
            eprintln!("Service cleanup could not be confirmed after 30 s: {error}. Exiting.");
            std::process::exit(1);
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
