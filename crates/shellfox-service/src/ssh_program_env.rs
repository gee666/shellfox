//! Convert the finite broker descriptor into terminal's opaque target-only capability.
//! Never feed internal callback keys through ordinary user PtyConfig.env.
use super::{
    native::{SshProgramPolicy, SshProgramRole},
    Result,
};
use anyhow::{ensure, Context};
use shellfox_terminal::{AskpassBroker, TrustedProgramEnv, TrustedProgramLease};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
struct Lease {
    path: PathBuf,
    role: SshProgramRole,
    policy: Arc<dyn SshProgramPolicy>,
}
impl TrustedProgramLease for Lease {
    fn path(&self) -> &Path {
        &self.path
    }
    fn revalidate(&self) -> Result<()> {
        self.policy.validate_current(self.role, &self.path)
    }
}
pub(super) fn capability(
    executable: &Path,
    local: &BTreeMap<String, String>,
    policy: Arc<dyn SshProgramPolicy>,
) -> Result<TrustedProgramEnv> {
    const KEYS: [&str; 6] = [
        "SSH_ASKPASS",
        "SSH_ASKPASS_REQUIRE",
        "DISPLAY",
        "SHELLFOX_SSH_ASKPASS_ENDPOINT",
        "SHELLFOX_SSH_ASKPASS_DIRECTORY",
        "SHELLFOX_SSH_ASKPASS_TOKEN",
    ];
    ensure!(
        local.len() == KEYS.len() && local.keys().all(|key| KEYS.contains(&key.as_str())),
        "Unexpected native callback environment key."
    );
    let value = |key: &str| {
        local
            .get(key)
            .context("Required native callback field missing")
    };
    ensure!(
        value("SSH_ASKPASS_REQUIRE")? == "force" && value("DISPLAY")? == "shellfox-askpass:0",
        "Unexpected askpass invocation mode."
    );
    let broker = AskpassBroker::new(
        PathBuf::from(value("SHELLFOX_SSH_ASKPASS_DIRECTORY")?),
        value("SHELLFOX_SSH_ASKPASS_ENDPOINT")?.clone(),
        value("SHELLFOX_SSH_ASKPASS_TOKEN")?.clone(),
    )?;
    let program = Arc::new(Lease {
        path: executable.to_owned(),
        role: SshProgramRole::Ssh,
        policy: policy.clone(),
    });
    let askpass = Arc::new(Lease {
        path: PathBuf::from(value("SSH_ASKPASS")?),
        role: SshProgramRole::Askpass,
        policy,
    });
    TrustedProgramEnv::ssh_askpass(program, askpass, broker)
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Exact {
        path: PathBuf,
        askpass: bool,
    }
    impl SshProgramPolicy for Exact {
        fn validate_current(&self, role: SshProgramRole, path: &Path) -> Result<()> {
            ensure!(
                path == self.path && (role != SshProgramRole::Askpass || self.askpass),
                "Wrong fixture program or role"
            );
            Ok(())
        }
    }
    #[test]
    fn finite_carrier_rejects_unexpected_keys_and_wrong_program_policy() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tmp");
        let directory = tempfile::Builder::new()
            .prefix("ssh-env-fixture-")
            .tempdir_in(root)
            .unwrap();
        let exe = std::env::current_exe().unwrap();
        let endpoint = if cfg!(windows) {
            format!(r"\\.\pipe\shellfox-rust-v1-{}", uuid::Uuid::new_v4())
        } else {
            directory
                .path()
                .join("broker.sock")
                .to_string_lossy()
                .into_owned()
        };
        let mut env = BTreeMap::from([
            ("SSH_ASKPASS".into(), exe.to_string_lossy().into_owned()),
            ("SSH_ASKPASS_REQUIRE".into(), "force".into()),
            ("DISPLAY".into(), "shellfox-askpass:0".into()),
            ("SHELLFOX_SSH_ASKPASS_ENDPOINT".into(), endpoint),
            (
                "SHELLFOX_SSH_ASKPASS_DIRECTORY".into(),
                directory.path().to_string_lossy().into_owned(),
            ),
            ("SHELLFOX_SSH_ASKPASS_TOKEN".into(), "0".repeat(64)),
        ]);
        let before = std::env::var_os("SHELLFOX_SSH_ASKPASS_TOKEN");
        assert!(
            capability(
                &exe,
                &env,
                Arc::new(Exact {
                    path: exe.clone(),
                    askpass: false
                })
            )
            .is_err(),
            "same resource cannot infer Askpass role"
        );
        let policy = Arc::new(Exact {
            path: exe.clone(),
            askpass: true,
        });
        assert!(capability(&exe, &env, policy.clone()).is_ok());
        env.insert("SHELLFOX_OWNERSHIP_MARKER".into(), "forbidden".into());
        assert!(capability(&exe, &env, policy.clone()).is_err());
        env.remove("SHELLFOX_OWNERSHIP_MARKER");
        assert!(capability(&directory.path().join("different.exe"), &env, policy).is_err());
        assert_eq!(std::env::var_os("SHELLFOX_SSH_ASKPASS_TOKEN"), before);
    }
}
