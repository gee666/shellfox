//! Real native-fixture PTY adapter, used by tests only.
//! Environment/profile selection stays in the host. This adapter accepts its
//! validated builder, replaces only the executable and reserved ownership keys.
mod owner;
use super::Lifecycle;
use anyhow::{ensure, Context, Result};
use owner::{NativeOwner, PlatformOwner};
use portable_pty::{CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

pub struct PreparedLaunch {
    owner: NativeOwner,
    program: std::path::PathBuf,
    args: Vec<std::ffi::OsString>,
    env: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
}
impl PreparedLaunch {
    #[cfg(target_os = "linux")]
    pub fn linux(
        runtime: &std::path::Path,
        supervisor: &std::path::Path,
        command: &std::path::Path,
        args: &[std::ffi::OsString],
        marker: &str,
    ) -> Result<Self> {
        let mut owner = super::linux::LinuxOwner::preflight(marker)?;
        owner.expect_image(command)?;
        let gate = super::linux::gate::StartupGate::prepare(runtime)?;
        let true_program = ["/usr/bin/true", "/bin/true"]
            .into_iter()
            .map(std::path::PathBuf::from)
            .find(|p| p.is_file());
        let runner = match true_program {
            Some(program) => super::linux::scope::discover(
                &std::env::var_os("PATH").unwrap_or_default(),
                &program,
            )
            .ok()
            .flatten(),
            None => None,
        };
        let argv = if let Some(runner) = runner {
            let scope_args = super::linux::scope::scope_arguments(
                &super::linux::scope::fresh_nonce()?,
                command,
                args,
            )?;
            gate.arguments(&runner, &scope_args)?
        } else {
            gate.arguments(command, args)?
        };
        let supervisor = super::private::helper_path(supervisor)?;
        Ok(Self {
            owner: NativeOwner {
                platform: PlatformOwner::Linux { owner, gate },
                child: None,
                status: None,
            },
            program: supervisor,
            args: argv,
            env: std::collections::BTreeMap::from([(
                "SHELLFOX_SUPERVISOR_MARKER".into(),
                marker.into(),
            )]),
        })
    }
    #[cfg(target_os = "macos")]
    pub fn darwin(
        runtime: &std::path::Path,
        helper: &std::path::Path,
        command: &std::path::Path,
        args: &[std::ffi::OsString],
        marker: &str,
        arch: &str,
    ) -> Result<Self> {
        let owner = super::darwin::PreparedSupervisor::prepare(helper, runtime, arch)?;
        let launch = owner.launch(command, args, marker)?;
        Ok(Self {
            owner: NativeOwner {
                platform: PlatformOwner::Darwin(owner),
                child: None,
                status: None,
            },
            program: launch.file,
            args: launch.args,
            env: launch.env,
        })
    }

    fn command_builder(
        &self,
        cwd: &std::path::Path,
        env: &std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
    ) -> Result<CommandBuilder> {
        ensure!(
            cwd.is_absolute() && cwd.is_dir(),
            "native PTY requires existing absolute isolated cwd"
        );
        let mut argv = vec![self.program.as_os_str().to_owned()];
        argv.extend_from_slice(&self.args);
        let mut command = CommandBuilder::from_argv(argv);
        command.env_clear();
        command.cwd(cwd);
        for (key, value) in env {
            command.env(key, value);
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }
        Ok(command)
    }
}

pub struct NativeSession {
    lifecycle: Option<Lifecycle<NativeOwner>>,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    reaper: Sender<Retained>,
}
struct Retained {
    lifecycle: Lifecycle<NativeOwner>,
    _master: Option<Box<dyn MasterPty + Send>>,
}
/// Startup errors carry the session whenever cleanup was not confirmed.
pub struct NativeSpawnError {
    pub error: anyhow::Error,
    pub retained: Option<Box<NativeSession>>,
}
impl std::fmt::Debug for NativeSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeSpawnError")
            .field("error", &self.error)
            .field("retained", &self.retained.is_some())
            .finish()
    }
}
impl NativeSession {
    pub fn spawn(
        launch: PreparedLaunch,
        cwd: &std::path::Path,
        env: &std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
        size: PtySize,
    ) -> std::result::Result<Self, NativeSpawnError> {
        let builder = launch
            .command_builder(cwd, env)
            .map_err(|error| NativeSpawnError {
                error,
                retained: None,
            })?;
        Self::spawn_inner(launch, builder, size)
            .map_err(|(error, retained)| NativeSpawnError { error, retained })
    }
    fn spawn_inner(
        launch: PreparedLaunch,
        builder: CommandBuilder,
        size: PtySize,
    ) -> std::result::Result<Self, (anyhow::Error, Option<Box<Self>>)> {
        let mut lifecycle = Lifecycle::prepare(launch.owner).map_err(|e| (e, None))?;
        let (sender, receiver) = mpsc::channel::<Retained>();
        std::thread::Builder::new()
            .name("native-pty-owner".into())
            .spawn(move || {
                while let Ok(mut retained) = receiver.recv() {
                    while retained.lifecycle.close().is_err() {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            })
            .map_err(|e| (e.into(), None))?;
        let mut writer = None;
        let master = lifecycle
            .allocate_pty(|owner| {
                ensure!(
                    size.cols > 0 && size.rows > 0 && size.cols <= 32767 && size.rows <= 32767,
                    "invalid PTY dimensions"
                );
                let pair = portable_pty::native_pty_system().openpty(size)?;
                writer = Some(pair.master.take_writer()?);
                let child = pair.slave.spawn_command(builder)?;
                owner.child = Some(child);
                drop(pair.slave);
                Ok(pair.master)
            })
            .map_err(|e| (e, None))?;
        let mut session = Self {
            lifecycle: Some(lifecycle),
            master: Some(master),
            writer,
            reaper: sender,
        };
        let ready = session
            .lifecycle
            .as_mut()
            .context("lifecycle missing")
            .and_then(Lifecycle::authenticate);
        if let Err(error) = ready {
            return match session.close() {
                Ok(()) => Err((error, None)),
                Err(cleanup) => Err((
                    error.context(format!("cleanup unconfirmed: {cleanup:#}")),
                    Some(Box::new(session)),
                )),
            };
        }
        Ok(session)
    }
    pub fn reader(&self) -> Result<Box<dyn Read + Send>> {
        self.master
            .as_ref()
            .context("PTY closed")?
            .try_clone_reader()
    }
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            self.lifecycle
                .as_ref()
                .is_some_and(Lifecycle::input_allowed),
            "PTY input disabled"
        );
        let result = self
            .writer
            .as_mut()
            .context("PTY writer closed")?
            .write_all(bytes);
        if let Err(error) = result {
            if let Some(lifecycle) = self.lifecycle.as_mut() {
                lifecycle.input_failed();
            }
            return Err(error.into());
        }
        Ok(())
    }
    pub fn resize(&self, size: PtySize) -> Result<()> {
        ensure!(
            self.lifecycle
                .as_ref()
                .is_some_and(Lifecycle::input_allowed),
            "PTY resize disabled"
        );
        ensure!(
            size.cols > 0 && size.rows > 0 && size.cols <= 32767 && size.rows <= 32767,
            "invalid PTY dimensions"
        );
        self.master.as_ref().context("PTY closed")?.resize(size)
    }
    pub fn poll(&mut self) -> Result<bool> {
        self.lifecycle.as_mut().context("PTY closed")?.poll()
    }
    pub fn close(&mut self) -> Result<()> {
        if let Some(lifecycle) = self.lifecycle.as_mut() {
            lifecycle.close()?;
        }
        self.lifecycle = None;
        self.writer = None;
        self.master = None;
        Ok(())
    }
}
impl Drop for NativeSession {
    fn drop(&mut self) {
        if let Some(lifecycle) = self.lifecycle.take() {
            let retained = Retained {
                lifecycle,
                _master: self.master.take(),
            };
            if let Err(mpsc::SendError(retained)) = self.reaper.send(retained) {
                std::mem::forget(retained);
            }
        }
    }
}
