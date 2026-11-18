use crate::{Error, NoDynamicCodeLoad, Result, VerifiedExecutable};
use std::{
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus},
    sync::{Mutex, OnceLock},
};

/// This wrapper retains the whole bundle through spawn and confirmed exit.
/// Callers still validate live protocol responses and sanitize child environment.
pub struct VerifiedCommand {
    command: Command,
    executable: VerifiedExecutable,
    loader_policy: Option<NoDynamicCodeLoad>,
}
impl VerifiedCommand {
    pub(crate) fn new(
        executable: VerifiedExecutable,
        loader_policy: Option<NoDynamicCodeLoad>,
    ) -> Result<Self> {
        if let Some(policy) = loader_policy {
            policy.verify(executable.bundle().expected_manifest_digest())?;
        }
        let path = executable.checked_native_path()?;
        let mut command = Command::new(path);
        command.current_dir(
            path.parent()
                .ok_or(Error::Manifest("entrypoint parent missing"))?,
        );
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW, child only.
        }
        Ok(Self {
            command,
            executable,
            loader_policy,
        })
    }
    pub fn command_mut(&mut self) -> &mut Command {
        &mut self.command
    }
    pub fn spawn(mut self) -> Result<VerifiedChild> {
        let expected = self.executable.checked_native_path()?;
        if self.command.get_program() != expected.as_os_str()
            || self.command.get_current_dir() != expected.parent()
        {
            return Err(Error::Manifest(
                "verified command program or working directory was replaced",
            ));
        }
        if self.executable.bundle().identity().id == crate::TrustedBundleId::WindowsManagedHelper {
            if let Some(policy) = self.loader_policy {
                policy.verify(self.executable.bundle().expected_manifest_digest())?;
            } else {
                let arguments: Vec<_> = self.command.get_args().collect();
                let expected = self
                    .executable
                    .protocol()
                    .id
                    .safe_probe_argument()
                    .ok_or(Error::LoaderPolicyRequired)?;
                if arguments.as_slice() != [std::ffi::OsStr::new(expected)] {
                    return Err(Error::LoaderPolicyRequired);
                }
            }
            // A complete publish hash cannot authenticate a profiler or startup
            // hook injected through an unrelated path in the child environment.
            let keys: Vec<_> = std::env::vars_os()
                .map(|(key, _)| key)
                .chain(self.command.get_envs().map(|(key, _)| key.to_owned()))
                .filter(|key| {
                    let key = key.to_string_lossy().to_ascii_uppercase();
                    ["DOTNET_", "COMPLUS_", "CORECLR_", "COR_"]
                        .iter()
                        .any(|prefix| key.starts_with(prefix))
                        || key == "DEVPATH"
                })
                .collect();
            for key in keys {
                self.command.env_remove(key);
            }
        }
        let child = self.command.spawn()?;
        Ok(VerifiedChild {
            child: Some(child),
            executable: Some(self.executable),
        })
    }
}

pub struct VerifiedChild {
    child: Option<Child>,
    executable: Option<VerifiedExecutable>,
}
impl VerifiedChild {
    pub fn id(&self) -> u32 {
        self.child.as_ref().map(Child::id).unwrap_or(0)
    }
    pub fn stdin_mut(&mut self) -> Option<&mut ChildStdin> {
        self.child.as_mut()?.stdin.as_mut()
    }
    pub fn stdout_mut(&mut self) -> Option<&mut ChildStdout> {
        self.child.as_mut()?.stdout.as_mut()
    }
    pub fn stderr_mut(&mut self) -> Option<&mut ChildStderr> {
        self.child.as_mut()?.stderr.as_mut()
    }
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        let status = self
            .child
            .as_mut()
            .ok_or(Error::Manifest("child missing"))?
            .try_wait()?;
        if status.is_some() {
            self.executable.take();
        }
        Ok(status)
    }
    pub fn wait(&mut self) -> Result<ExitStatus> {
        let status = self
            .child
            .as_mut()
            .ok_or(Error::Manifest("child missing"))?
            .wait()?;
        self.executable.take();
        Ok(status)
    }
}

struct Unconfirmed {
    child: Child,
    _executable: VerifiedExecutable,
}
static UNCONFIRMED: OnceLock<Mutex<Vec<Unconfirmed>>> = OnceLock::new();

impl Drop for VerifiedChild {
    fn drop(&mut self) {
        let (Some(mut child), Some(executable)) = (self.child.take(), self.executable.take())
        else {
            return;
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        // Dropping an unconfirmed owner must not unlock its lazy-load payload.
        // No thread creation failure or timeout can release this in-process hold.
        UNCONFIRMED
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(Unconfirmed {
                child,
                _executable: executable,
            });
    }
}

/// Release quarantined bundle handles only when the OS confirms helper exit.
/// Errors retain ownership. This never signals or kills any process.
pub fn reap_unconfirmed_children() -> usize {
    let mut owners = UNCONFIRMED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut index = 0;
    let mut reaped = 0;
    while index < owners.len() {
        if matches!(owners[index].child.try_wait(), Ok(Some(_))) {
            owners.swap_remove(index);
            reaped += 1;
        } else {
            index += 1;
        }
    }
    reaped
}
