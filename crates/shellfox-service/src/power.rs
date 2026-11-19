//! Host-owned keep-awake requests. No request is made until explicitly enabled.
//! Platform calls can block, so run them on the service's blocking worker.
use std::{error::Error, fmt};
#[cfg(test)]
#[path = "power_linux_tests.rs"]
mod linux_tests;
#[path = "power_contract.rs"]
mod contract;
pub use contract::{PowerReport, PowerRequest, MAX_POWER_MESSAGE_BYTES};
#[path = "power_worker.rs"]
pub(crate) mod worker;

#[cfg(target_os = "linux")]
#[path = "power_linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "power_macos.rs"]
mod platform;
#[cfg(windows)]
#[path = "power_windows.rs"]
mod platform;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
#[path = "power_unsupported.rs"]
mod platform;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerError(pub String);
impl fmt::Display for PowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl Error for PowerError {}
impl PowerError {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub(crate) fn operation(operation: &str, error: impl fmt::Display) -> Self {
        Self(format!("{operation}: {error}"))
    }
}

/// Owns exactly the platform requests created by one successful acquisition.
/// `release` is retryable; dropping a lease is the last-resort cleanup.
pub trait PowerLease: Send {
    fn release(&mut self) -> Result<(), PowerError>;
    /// Explains any requested baseline behavior that this lease does not provide.
    fn partial_reason(&self) -> Option<&str> {
        None
    }
}
pub trait PowerBackend: Send {
    type Lease: PowerLease;
    fn acquire(&mut self) -> Result<Self::Lease, PowerError>;
}

#[cfg(any(target_os = "linux", test))]
pub(crate) struct LinuxInhibitors<T> {
    pub primary: T,
    pub lid: Option<T>,
    pub partial_reason: Option<String>,
}
#[cfg(any(target_os = "linux", test))]
pub(crate) fn acquire_linux_inhibitors<T>(
    mut acquire: impl FnMut(&str) -> Result<T, PowerError>,
) -> Result<LinuxInhibitors<T>, PowerError> {
    let primary = acquire("idle:sleep")?;
    let (lid, partial_reason) = match acquire("handle-lid-switch") {
        Ok(lid) => (Some(lid), None),
        Err(error) => (
            None,
            Some(format!("Lid-switch inhibitor unavailable: {error}")),
        ),
    };
    Ok(LinuxInhibitors {
        primary,
        lid,
        partial_reason,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerStatus {
    Inactive,
    Active,
    Partial,
    ReleaseFailed,
}

#[derive(Default)]
pub struct SystemPower {
    #[cfg(windows)]
    journal: Option<std::path::PathBuf>,
}
impl SystemPower {
    pub(crate) fn for_data_dir(directory: &std::path::Path) -> Self {
        #[cfg(windows)]
        {
            let journal = platform::prepare(directory);
            Self { journal: Some(journal) }
        }
        #[cfg(not(windows))]
        {
            let _ = directory;
            Self::default()
        }
    }
}
impl PowerBackend for SystemPower {
    type Lease = platform::Lease;
    fn acquire(&mut self) -> Result<Self::Lease, PowerError> {
        #[cfg(windows)]
        return platform::acquire(self.journal.as_deref());
        #[cfg(not(windows))]
        platform::acquire()
    }
}

pub struct PowerManager<B: PowerBackend = SystemPower> {
    backend: B,
    lease: Option<B::Lease>,
    release_failed: bool,
}
impl PowerManager<SystemPower> {
    pub fn new() -> Self {
        Self::with_backend(SystemPower::default())
    }
}
impl Default for PowerManager<SystemPower> {
    fn default() -> Self {
        Self::new()
    }
}
impl<B: PowerBackend> PowerManager<B> {
    pub fn with_backend(backend: B) -> Self {
        Self {
            backend,
            lease: None,
            release_failed: false,
        }
    }
    pub fn status(&self) -> PowerStatus {
        match (&self.lease, self.release_failed) {
            (None, _) => PowerStatus::Inactive,
            (Some(lease), false) if lease.partial_reason().is_some() => PowerStatus::Partial,
            (Some(_), false) => PowerStatus::Active,
            (Some(_), true) => PowerStatus::ReleaseFailed,
        }
    }
    pub fn partial_reason(&self) -> Option<&str> {
        self.lease.as_ref().and_then(PowerLease::partial_reason)
    }
    /// Returns the confirmed state. A failed release retains the lease for retry.
    pub fn set(&mut self, active: bool) -> Result<PowerStatus, PowerError> {
        if active {
            if self.release_failed {
                return Err(PowerError(
                    "Keep-awake release needs retry before enabling.".into(),
                ));
            }
            if self.lease.is_none() {
                self.lease = Some(self.backend.acquire()?);
            }
        } else if let Some(lease) = self.lease.as_mut() {
            if let Err(error) = lease.release() {
                self.release_failed = true;
                return Err(error);
            }
            self.lease = None;
            self.release_failed = false;
        }
        Ok(self.status())
    }
}
