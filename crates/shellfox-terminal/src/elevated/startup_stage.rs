pub(super) const DENIED: &str = "Windows administrator approval was denied or dismissed.";
pub(super) const WAIT_CANCELLED: &str = "Administrator PowerShell was cancelled while waiting for Windows approval.";
const STARTED: &str = "Elevated helper started but failed at ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupStage {
    Connect,
    VerifyHelper,
    SendLaunch,
    Ready,
    Elevation,
    OpenLauncher,
    ConnectToLauncher,
    VerifyLauncher,
    ReadLaunch,
    CreatePowerShell,
    SendReady,
}

impl StartupStage {
    const LABELS: &[(Self, &str)] = &[
        (Self::Connect, "connecting to the elevated helper"),
        (Self::VerifyHelper, "authenticating the elevated helper"),
        (Self::SendLaunch, "sending the launch request"),
        (Self::Ready, "waiting for READY"),
        (Self::Elevation, "checking elevation"),
        (Self::OpenLauncher, "opening the launcher process"),
        (Self::ConnectToLauncher, "connecting to the launcher pipe"),
        (Self::VerifyLauncher, "authenticating the launcher"),
        (Self::ReadLaunch, "reading the launch request in the helper"),
        (Self::CreatePowerShell, "creating the PowerShell ConPTY"),
        (Self::SendReady, "sending READY"),
    ];

    pub(super) fn from_message(message: &str) -> Option<Self> {
        let label = message.strip_prefix(STARTED)?.strip_suffix('.')?;
        Self::LABELS
            .iter()
            .find_map(|&(stage, allowed)| (allowed == label).then_some(stage))
    }
}

impl std::fmt::Display for StartupStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = Self::LABELS
            .iter()
            .find_map(|&(candidate, label)| (candidate == *self).then_some(label))
            .expect("known startup stage");
        write!(f, "{STARTED}{label}.")
    }
}
