//! Read-only discovery. The configured supervisor path is never executed here.
use super::SourceFacts;
use shellfox_protocol as wire;
use shellfox_protocol::Validate;
use shellfox_terminal::{discover_shell_profiles, ShellProfileId};
use std::path::{Path, PathBuf};

pub struct DiscoveredFacts {
    pub probe: wire::NativeProbe,
    pub profiles: wire::TerminalProfiles,
}
pub trait FactsDiscovery: Send + Sync {
    fn discover(&self, source: &SourceFacts) -> Result<DiscoveredFacts, wire::AppError>;
}
pub struct NativeFactsDiscovery {
    supervisor: PathBuf,
}
impl NativeFactsDiscovery {
    pub fn new(supervisor: PathBuf) -> anyhow::Result<Self> {
        anyhow::ensure!(
            supervisor.is_absolute() && supervisor.is_file(),
            "Explicit existing supervisor resource path required for discovery."
        );
        Ok(Self { supervisor })
    }
}
pub(super) fn unavailable(capability: &'static str) -> wire::AppError {
    shellfox_remote::adapter::AdapterError::CapabilityUnavailable { capability }.into()
}
impl FactsDiscovery for NativeFactsDiscovery {
    fn discover(&self, source: &SourceFacts) -> Result<DiscoveredFacts, wire::AppError> {
        let mut discovered = self.discover_catalog(
            &source.settings,
            &source.snapshot.preferences.default_profile_id,
            &source.cancelled,
        )?;
        discovered.probe.capabilities.close_terminal = !source.terminals.is_empty();
        Ok(discovered)
    }
}
impl NativeFactsDiscovery {
    /// Same read-only mapper for owner-native and remote fact discovery. Inputs
    /// are captured by the original owner, never supplied as GUI filesystem claims.
    pub fn discover_catalog(
        &self,
        settings: &shellfox_core::Settings,
        default: &shellfox_core::ProfileId,
        cancel: &wire::CancellationToken,
    ) -> Result<DiscoveredFacts, wire::AppError> {
        if cancel.is_cancelled() {
            return Err(unavailable("cancelled owner discovery"));
        }
        let catalog =
            discover_shell_profiles().map_err(|_| unavailable("native shell discovery"))?;
        // Engine::new checks the loaded public ABI version, not just a source pin.
        // The !Send engine is created and dropped entirely on this discovery thread.
        let engine = shellfox_ghostty_vt::Terminal::new(1, 1).is_ok();
        let supervisor_present = self.supervisor.is_file();
        let backend = cfg!(windows) && supervisor_present && engine;
        let mut profiles = Vec::new();
        let mut shells = Vec::new();
        let mut reasons = Vec::new();
        for profile in catalog {
            if cancel.is_cancelled() {
                return Err(unavailable("cancelled owner discovery"));
            }
            if let Some(reason) = profile.unavailable_reason() {
                reasons.push(format!("{}: {reason}", profile.label()));
            }
            let Some(path) = profile.executable().and_then(wire_path) else {
                continue;
            };
            let command = profile.command().ok();
            let available = backend && command.is_some();
            let reason = if let Some(reason) = profile.unavailable_reason() {
                Some(reason.to_owned())
            } else if !supervisor_present {
                Some("Configured supervisor resource is unavailable.".into())
            } else if !engine {
                Some("Public Ghostty terminal engine did not initialize.".into())
            } else if !cfg!(windows) {
                Some("The discovered profile has no wired native launch adapter.".into())
            } else if command.is_none() {
                Some("Discovered executable is no longer available.".into())
            } else {
                None
            };
            let args = command
                .map(|command| {
                    command
                        .args
                        .into_iter()
                        .map(|arg| {
                            arg.into_string()
                                .map_err(|_| unavailable("lossless shell arguments"))
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default();
            shells.push(wire::ShellOption {
                id: shell_id(profile.id()),
                executable: path.clone(),
                available,
                reason: reason.clone(),
            });
            profiles.push(wire::TerminalProfile {
                id: profile.id().as_str().into(),
                label: profile.label().into(),
                environment: if profile.id() == ShellProfileId::Wsl {
                    wire::ProfileEnvironment::Wsl
                } else {
                    wire::ProfileEnvironment::Local
                },
                executable: path,
                args,
                distro: None,
                available,
                unavailable_reason: Some(reason),
                can_terminate_descendants: Some(available),
                elevation: None,
            });
        }
        if !supervisor_present {
            reasons.push(
                "Configured supervisor resource is unavailable; no path was executed.".into(),
            );
        }
        if !engine {
            reasons.push("Public Ghostty terminal engine did not initialize.".into());
        }
        reasons.push("Incoming legacy v1 launch/view and full-state recovery are not advertised by this host.".into());
        let available = profiles.iter().any(|profile| profile.available);
        let configured = settings
            .terminal_profile_id
            .as_ref()
            .and_then(|value| value.clone())
            .unwrap_or_else(|| default.to_string());
        let default_profile_id = profiles
            .iter()
            .find(|profile| profile.id == configured && profile.available)
            .map(|profile| profile.id.clone());
        let profiles = wire::TerminalProfiles {
            profiles,
            default_profile_id,
            lifetime: wire::TerminalLifetime::AppOwned,
            shell_survival: false,
        };
        // Legacy v1 launch/view/full-state recovery remain gated in remote_commands.
        // These are protocol capability absences, not invented process observations.
        let probe = wire::NativeProbe {
            platform: match std::env::consts::OS {
                "windows" => "win32",
                "macos" => "darwin",
                os => os,
            }
            .into(),
            arch: match std::env::consts::ARCH {
                "x86_64" => "x64",
                "aarch64" => "arm64",
                arch => arch,
            }
            .into(),
            adapter_id: wire::AdapterId::EmbeddedPty,
            available,
            python: None,
            terminal_version: engine.then(|| "libghostty-vt 1.0.1".into()),
            capabilities: wire::Capabilities {
                create_window: false,
                add_tab: false,
                focus_window: false,
                activate_tab: false,
                split_pane: false,
                attach_existing: false,
                // Catalog-only discovery grants no runtime-target operation.
                close_terminal: false,
                command_exit_status: false,
                process_tracking: cfg!(windows),
                explorer_context_menu: false,
                embedded_terminal: Some(engine),
                terminal_lifetime: Some(wire::TerminalLifetime::AppOwned),
                shell_survival: Some(false),
            },
            shells,
            reasons,
        };
        probe
            .validate()
            .map_err(|_| unavailable("validated native probe"))?;
        profiles
            .validate()
            .map_err(|_| unavailable("validated shell catalog"))?;
        Ok(DiscoveredFacts { probe, profiles })
    }
}
fn wire_path(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    let text = text.strip_prefix("\\\\?\\").unwrap_or(text);
    wire::validation::cwd(text).ok()?;
    Some(text.into())
}
fn shell_id(profile: ShellProfileId) -> wire::ShellId {
    match profile {
        ShellProfileId::Pwsh => wire::ShellId::Pwsh,
        ShellProfileId::WindowsPowerShell => wire::ShellId::WindowsPowershell,
        ShellProfileId::Wsl => wire::ShellId::Wsl,
        ShellProfileId::Cmd | ShellProfileId::LoginShell => wire::ShellId::LoginShell,
    }
}
