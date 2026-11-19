//! Fault injection compiled only with the explicit fixture feature.
use crate::{PtyConfig, PtySession, SpawnError};

#[derive(Clone, Copy)]
pub enum StartupFault {
    /// Inject an assignment error before GO. No command may have started.
    AssignmentDenied,
    /// Never signal GO. The bounded startup wait must fail closed.
    NoRelease,
    /// Inject a post-READY error and a cleanup error, returning retained ownership.
    ReadyCleanupFailure,
}

impl PtySession {
    pub fn spawn_with_test_fault(
        config: PtyConfig,
        fault: StartupFault,
    ) -> Result<Self, SpawnError> {
        #[cfg(windows)]
        {
            let options = crate::session::SpawnOptions {
                deny_assignment: matches!(fault, StartupFault::AssignmentDenied),
                skip_release: matches!(fault, StartupFault::NoRelease),
                fail_ready_cleanup: matches!(fault, StartupFault::ReadyCleanupFailure),
            };
            Self::spawn_windows(config, options)
        }
        #[cfg(target_os = "linux")]
        {
            Self::spawn_inner(
                config,
                None,
                crate::unix::session::SpawnOptions { fault: Some(fault) },
            )
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = fault;
            Self::spawn(config)
        }
    }

    pub fn set_cleanup_failure_for_test(&mut self, fail: bool) {
        #[cfg(windows)]
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_cleanup_failure = fail;
        }
        #[cfg(target_os = "linux")]
        self.set_cleanup_failure(fail);
        #[cfg(not(any(windows, target_os = "linux")))]
        let _ = fail;
    }
}
