use super::stdio::StdioIo;
use super::{
    AdapterError, Capability, Generation, HelperFrame, HelperIo, PreparedLaunch, Session,
    StartError,
};
use shellfox_resources::{
    NoDynamicCodeLoad, ProtocolId, RuntimeId, TrustedBundleId, VerifiedExecutable,
};
use std::{
    process::Command,
    time::{Duration, Instant},
};

/// A fresh trusted-host UI approval action. It is consumed by one generation;
/// do not deserialize it from renderer state or infer it from a saved setting.
pub struct UserApproval {
    generation: Generation,
}
impl UserApproval {
    pub fn from_user_action(generation: Generation) -> Self {
        Self { generation }
    }
}

/// Resource-backed factory. A matching hash/probe cannot create loader policy.
/// None remains the default while the managed loader review is ungranted.
pub struct NativeFactory {
    executable: VerifiedExecutable,
    loader_policy: Option<NoDynamicCodeLoad>,
}
impl NativeFactory {
    pub fn new(
        executable: VerifiedExecutable,
        loader_policy: Option<NoDynamicCodeLoad>,
    ) -> Result<Self, AdapterError> {
        let expected_rid = if cfg!(target_arch = "aarch64") {
            RuntimeId::WinArm64
        } else {
            RuntimeId::WinX64
        };
        if executable.bundle().identity().id != TrustedBundleId::WindowsManagedHelper
            || executable.bundle().identity().rid != expected_rid
            || executable.protocol().id != ProtocolId::ElevatedIsolated
            || executable.protocol().version != 1
        {
            return Err(AdapterError::InvalidResource);
        }
        Ok(Self {
            executable,
            loader_policy,
        })
    }
    pub fn loader_gate(&self) -> Result<(), AdapterError> {
        let policy = self
            .loader_policy
            .as_ref()
            .ok_or(AdapterError::LoaderPolicyRequired)?;
        self.executable
            .path_for_spawn_with_loader_policy(policy)
            .map_err(|_| AdapterError::InvalidResource)?;
        Ok(())
    }
    fn command(&self, argument: &str, launch: &PreparedLaunch) -> Result<Command, AdapterError> {
        let policy = self
            .loader_policy
            .as_ref()
            .ok_or(AdapterError::LoaderPolicyRequired)?;
        let path = self
            .executable
            .path_for_spawn_with_loader_policy(policy)
            .map_err(|_| AdapterError::InvalidResource)?;
        let mut command = Command::new(path);
        command
            .arg(argument)
            .current_dir(path.parent().ok_or(AdapterError::InvalidResource)?);
        command.env_clear().envs(launch.launcher_environment());
        Ok(command)
    }
    /// Bounded live validation on the same pinned full bundle. This method does
    /// not request UAC. It still refuses until explicit loader review is supplied.
    pub fn probe(&self, launch: &PreparedLaunch) -> Result<Capability, AdapterError> {
        let command = self.command("probe-isolated", launch)?;
        let mut io =
            StdioIo::spawn(command, Box::new(self.executable.clone())).map_err(|failure| {
                drop(failure.retained);
                failure.error
            })?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut capability = None;
        loop {
            match io.try_receive_line().map_err(|_| AdapterError::Transport)? {
                Some(HelperFrame::Line(line)) if capability.is_none() => {
                    let parsed = Capability::parse_probe(&line)?;
                    parsed.require_isolated()?;
                    capability = Some(parsed);
                }
                Some(HelperFrame::Eof) => break,
                Some(_) => return Err(AdapterError::InvalidProtocol),
                None => {}
            }
            if Instant::now() >= deadline {
                return Err(AdapterError::Transport);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        io.finish(Duration::from_secs(2))?;
        capability.ok_or(AdapterError::InvalidProtocol)
    }
    pub fn launch(
        &self,
        approval: UserApproval,
        generation: Generation,
        launch: PreparedLaunch,
    ) -> Result<Session, StartError> {
        if approval.generation != generation {
            return Err(StartError {
                error: AdapterError::StaleGeneration,
                retained: None,
            });
        }
        self.probe(&launch).map_err(|error| StartError {
            error,
            retained: None,
        })?;
        let command = self
            .command("launch-isolated", &launch)
            .map_err(|error| StartError {
                error,
                retained: None,
            })?;
        // This extra executable clone owns Arc<VerifiedBundle> independently of
        // the launcher's Child. Session releases it only after peer proof AND
        // original launcher/stream cleanup, including failed-start retention.
        Session::start(
            command,
            Box::new(self.executable.clone()),
            generation,
            launch,
        )
    }
}
