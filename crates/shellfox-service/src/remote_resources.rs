//! Planned production state is descriptive until the retained native owner prepares it.
use super::*;
use shellfox_transport::{IsolatedStateDirectory, PermissionVerifier};
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};
#[derive(Clone)]
pub struct RemoteResources {
    pub state_directory: IsolatedStateDirectory,
    pub hostname: String,
    pub network_factory: Arc<dyn runtime::RemoteNetworkFactory>,
}
impl RemoteResources {
    pub fn go_with_control_server(
        state_directory: IsolatedStateDirectory,
        hostname: String,
        control: shellfox_transport::LoopbackOrigin,
    ) -> Self {
        Self {
            state_directory,
            hostname,
            network_factory: runtime::GoNetworkFactory::with_control_server(control),
        }
    }
    pub fn go(state_directory: IsolatedStateDirectory, hostname: String) -> Self {
        Self {
            state_directory,
            hostname,
            network_factory: Arc::new(runtime::GoNetworkFactory),
        }
    }
}
pub enum RemoteBootResources {
    Prepared(RemoteResources),
    Planned {
        state_directory: PathBuf,
        hostname: String,
        control_url: Option<shellfox_transport::LoopbackOrigin>,
    },
}
impl From<RemoteResources> for RemoteBootResources {
    fn from(resources: RemoteResources) -> Self {
        Self::Prepared(resources)
    }
}
impl RemoteBootResources {
    /// Read-only OS hostname query, not a guessed package/install identity.
    pub fn planned_for_current_host(state_directory: PathBuf) -> anyhow::Result<Self> {
        let hostname = native_hostname().unwrap_or_default();
        Self::planned(state_directory, default_hostname(&hostname))
    }
    /// No filesystem queries, creation, permissions, node or helper startup.
    pub fn planned(state_directory: PathBuf, hostname: String) -> anyhow::Result<Self> {
        Self::plan_control(state_directory, hostname, None)
    }
    pub fn planned_with_control_server(
        state_directory: PathBuf,
        hostname: String,
        control: shellfox_transport::LoopbackOrigin,
    ) -> anyhow::Result<Self> {
        Self::plan_control(state_directory, hostname, Some(control))
    }
    fn plan_control(
        state_directory: PathBuf,
        hostname: String,
        control_url: Option<shellfox_transport::LoopbackOrigin>,
    ) -> anyhow::Result<Self> {
        let state_directory = shellfox_transport::normalize_private_path(&state_directory)?;
        anyhow::ensure!(
            state_directory.parent().is_some(),
            "Network state cannot be a filesystem root"
        );
        super::super::native::RemoteRequest::Configure {
            enabled: false,
            hostname: hostname.clone(),
        }
        .validate()
        .map_err(|error| anyhow::anyhow!(error.message))?;
        Ok(Self::Planned {
            state_directory,
            hostname,
            control_url,
        })
    }
}
impl RemoteHostSetup {
    pub(super) async fn prepare_resources(
        &mut self,
        resources: RemoteBootResources,
        control_path: &Path,
        verifier: &PermissionVerifier,
    ) -> Result<(), AppError> {
        if self.resources.is_some() || self.state_attempted || self.lost.is_cancelled() {
            return Err(unavailable());
        }
        match resources {
            RemoteBootResources::Prepared(resources) => {
                super::super::native::RemoteRequest::Configure {
                    enabled: false,
                    hostname: resources.hostname.clone(),
                }
                .validate()?;
                self.resources = Some(resources);
            }
            RemoteBootResources::Planned {
                state_directory,
                hostname,
                control_url,
            } => {
                // Public enum fields are not validation proof; reject before effects.
                let state_directory = shellfox_transport::normalize_private_path(&state_directory)
                    .map_err(|_| unavailable())?;
                super::super::native::RemoteRequest::Configure {
                    enabled: false,
                    hostname: hostname.clone(),
                }
                .validate()?;
                let control_path = shellfox_transport::normalize_private_path(control_path)
                    .map_err(|_| unavailable())?;
                let relative = state_directory
                    .strip_prefix(&control_path)
                    .map_err(|_| unavailable())?;
                let parts: Vec<_> = relative
                    .components()
                    .map(|part| match part {
                        Component::Normal(name) => Ok(name.to_owned()),
                        _ => Err(unavailable()),
                    })
                    .collect::<Result<_, _>>()?;
                // A fixed private namespace under the ORIGINAL control cap; never
                // turn an arbitrary configured path into filesystem authority.
                if parts.len() < 2 || parts.len() > 16 || parts[0] != "go-network" {
                    return Err(unavailable());
                }
                self.planned_state = Some(state_directory);
                let state_directory = self.planned_state.as_ref().ok_or_else(unavailable)?.clone();
                self.network_parent = Some(self._control.files());
                for name in &parts[..parts.len() - 1] {
                    if self.lost.is_cancelled() {
                        return Err(unavailable());
                    }
                    let parent = self.network_parent.as_ref().ok_or_else(unavailable)?;
                    let child = parent
                        .open_or_create_private_child(name)
                        .map_err(|_| unavailable())?;
                    self.network_parent = Some(child);
                }
                self.keeper.verify_retained().map_err(|_| unavailable())?;
                self.state_attempted = true;
                let proof = match std::fs::symlink_metadata(&state_directory) {
                    Ok(info) if info.is_dir() && !info.file_type().is_symlink() => {
                        IsolatedStateDirectory::open_existing(state_directory, verifier).await
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        IsolatedStateDirectory::create_new_private(state_directory, verifier).await
                    }
                    _ => return Err(unavailable()),
                }
                .map_err(|_| unavailable())?;
                self.resources = Some(match control_url {
                    Some(control) => {
                        RemoteResources::go_with_control_server(proof, hostname, control)
                    }
                    None => RemoteResources::go(proof, hostname),
                });
                if self.lost.is_cancelled() {
                    return Err(unavailable());
                }
            }
        }
        Ok(())
    }
}
fn default_hostname(hostname: &str) -> String {
    let lowercase = hostname.to_lowercase();
    let suffix = lowercase.encode_utf16().map(|unit| match unit {
        0x30..=0x39 | 0x61..=0x7a => char::from_u32(u32::from(unit)).unwrap_or('-'),
        _ => '-',
    });
    "shellfox-"
        .chars()
        .chain(suffix)
        .take(63)
        .collect::<String>()
        .trim_end_matches('-')
        .to_owned()
}

fn native_hostname() -> anyhow::Result<String> {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetComputerNameW(buffer: *mut u16, size: *mut u32) -> i32;
        }
        let mut buffer = [0u16; 256];
        let mut size = 256u32;
        // SAFETY: read-only OS query into a bounded writable UTF-16 buffer.
        anyhow::ensure!(
            unsafe { GetComputerNameW(buffer.as_mut_ptr(), &mut size) } != 0 && size < 256,
            "OS hostname unavailable"
        );
        Ok(String::from_utf16(&buffer[..usize::try_from(size)?])?)
    }
    #[cfg(unix)]
    {
        extern "C" {
            fn gethostname(name: *mut std::os::raw::c_char, len: usize) -> std::os::raw::c_int;
        }
        let mut buffer = [0u8; 256];
        // SAFETY: POSIX read-only query; require a returned NUL within capacity.
        anyhow::ensure!(
            unsafe { gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } == 0,
            "OS hostname unavailable"
        );
        let length = buffer
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| anyhow::anyhow!("OS hostname truncated"))?;
        Ok(std::str::from_utf8(&buffer[..length])?.to_owned())
    }
    #[cfg(not(any(windows, unix)))]
    anyhow::bail!("OS hostname query unsupported");
}
#[cfg(test)]
mod tests {
    use super::default_hostname;

    #[test]
    fn default_hostname_matches_electron_dns_label() {
        assert_eq!(default_hostname("WORK.STATION"), "shellfox-work-station");
        assert_eq!(default_hostname("desk🦊01-"), "shellfox-desk--01");
        assert_eq!(default_hostname("---"), "shellfox");
        assert_eq!(default_hostname(&"A".repeat(70)).len(), 63);
    }
}

fn unavailable() -> AppError {
    failure(
        ErrorCode::NativeUnavailable,
        "Owned network state is unavailable; no repair, adoption or fallback was attempted.",
    )
}
