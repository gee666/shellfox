use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MANIFEST_NAME: &str = "bundle-manifest.json";
pub(crate) const MAX_MANIFEST: u64 = 2 * 1024 * 1024;
pub(crate) const MAX_FILE: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;
pub(crate) const MAX_FILES: usize = 2048;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TrustedBundleId {
    WindowsManagedHelper,
    GoTailscale,
    DarwinTerminal,
    WslSupervisor,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum RuntimeId {
    #[serde(rename = "win-x64")]
    WinX64,
    #[serde(rename = "win-arm64")]
    WinArm64,
    #[serde(rename = "linux-x64")]
    LinuxX64,
    #[serde(rename = "linux-arm64")]
    LinuxArm64,
    #[serde(rename = "osx-x64")]
    OsxX64,
    #[serde(rename = "osx-arm64")]
    OsxArm64,
}
impl RuntimeId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WinX64 => "win-x64",
            Self::WinArm64 => "win-arm64",
            Self::LinuxX64 => "linux-x64",
            Self::LinuxArm64 => "linux-arm64",
            Self::OsxX64 => "osx-x64",
            Self::OsxArm64 => "osx-arm64",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleIdentity {
    pub id: TrustedBundleId,
    pub rid: RuntimeId,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ResourceRuntime {
    #[serde(rename_all = "camelCase")]
    Dotnet {
        version: String,
        target_framework: String,
        entry_assembly: String,
        self_contained: bool,
    },
    Go {
        version: String,
    },
    Native {
        abi: String,
    },
    Python {
        major: u8,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum ProtocolId {
    ElevatedConpty,
    ElevatedIsolated,
    PowerGuardian,
    Tailscale,
    DarwinSnapshot,
    DarwinSupervisor,
    WslGuardian,
}

impl ProtocolId {
    pub const fn safe_probe_argument(self) -> Option<&'static str> {
        match self {
            Self::ElevatedConpty => Some("probe"),
            Self::ElevatedIsolated => Some("probe-isolated"),
            Self::PowerGuardian => Some("probe-power"),
            Self::Tailscale => Some("--probe"),
            Self::DarwinSnapshot | Self::DarwinSupervisor => Some("--capabilities"),
            Self::WslGuardian => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    Conpty,
    IsolatedWindowsPowerShell,
    NoProfile,
    Display,
    System,
    AwayMode,
    IdleMouse,
    LidRollback,
    EmbeddedNetwork,
    Ownership,
    PrivatePermissions,
    ExactBirth,
    IdentityAccess,
    Enumeration,
    Argv,
    EnvironmentMarker,
    OwnedSession,
    Guardians,
    Termination,
    Pidfd,
    Subreaper,
    Peercred,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProtocolContract {
    pub id: ProtocolId,
    pub version: u16,
    // Vec, rather than a serde set, lets validation reject duplicate claims.
    pub capabilities: Vec<Capability>,
}
impl ProtocolContract {
    pub fn capability_set(&self) -> BTreeSet<Capability> {
        self.capabilities.iter().copied().collect()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryPoint {
    pub path: String,
    pub protocols: Vec<ProtocolContract>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct FileSet(pub Vec<BundleFile>);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleManifest {
    pub format: String,
    pub identity: BundleIdentity,
    pub runtime: ResourceRuntime,
    pub entrypoints: Vec<EntryPoint>,
    pub files: FileSet,
}
