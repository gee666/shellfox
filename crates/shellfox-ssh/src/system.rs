//! Retained OpenSSH adapter. The host supplies its contained PTY/runtime factory.
//! Probe exchanges public keys only. Authentication runs with an exact private pin file.
use crate::{
    host_key_info,
    pty::{remote_environment, RemotePty},
    Error, HostKeyInfo, KnownHosts, PrivateDirectory, ProcessRequest, ProcessRunner, Result,
    Secret, StoredProfile, TrustDecision,
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct SshPrograms {
    pub ssh: PathBuf,
    pub keyscan: PathBuf,
}
impl SshPrograms {
    pub fn validate(&self) -> Result<()> {
        if [&self.ssh, &self.keyscan]
            .iter()
            .any(|p| !p.is_absolute() || !p.is_file())
        {
            return Err(Error::Process);
        }
        Ok(())
    }
}
/// Host-only capability. A factory must use an authenticated local askpass channel for
/// saved_password, never argv, logs, a public file, or an environment variable.
#[derive(Debug)]
pub struct HostAuthentication {
    pub saved_password: Option<Secret>,
}
pub use crate::system_proof::AuthenticationProof;

pub struct SshLaunchRequest {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub environment: BTreeMap<String, String>,
    pub pty: RemotePty,
    pub authentication: HostAuthentication,
    pub expected_authentication: AuthenticationProof,
}
impl std::fmt::Debug for SshLaunchRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshLaunchRequest")
            .field("executable", &self.executable)
            .field("pty", &self.pty)
            .finish_non_exhaustive()
    }
}
pub struct SystemSshAdapter {
    programs: SshPrograms,
    runner: Arc<dyn ProcessRunner>,
    directory: Arc<PrivateDirectory>,
    known: KnownHosts,
}
impl SystemSshAdapter {
    pub fn new(
        programs: SshPrograms,
        runner: Arc<dyn ProcessRunner>,
        directory: Arc<PrivateDirectory>,
    ) -> Result<Self> {
        programs.validate()?;
        Ok(Self {
            programs,
            runner,
            known: KnownHosts::new(directory.clone()),
            directory,
        })
    }
    pub fn ssh_executable(&self) -> &Path {
        &self.programs.ssh
    }
    pub fn keyscan_executable(&self) -> &Path {
        &self.programs.keyscan
    }
    pub fn uses_directory(&self, directory: &Arc<PrivateDirectory>) -> bool {
        Arc::ptr_eq(&self.directory, directory)
    }
    /// Explicit user-requested operation only. Never run during settings/list/startup.
    /// The public key scan is untrusted until confirmation and strict SSH verification.
    pub fn prepare_launch<'a>(
        &self,
        profile: &StoredProfile,
        pty: RemotePty,
        env: impl IntoIterator<Item = (&'a str, &'a str)>,
        confirm: impl FnOnce(&str, u16, &HostKeyInfo) -> Result<TrustDecision>,
    ) -> Result<SshLaunchRequest> {
        self.prepare_launch_guarded(profile, pty, env, confirm, || Ok(()))
    }
    pub fn prepare_launch_guarded<'a>(
        &self,
        profile: &StoredProfile,
        pty: RemotePty,
        env: impl IntoIterator<Item = (&'a str, &'a str)>,
        confirm: impl FnOnce(&str, u16, &HostKeyInfo) -> Result<TrustDecision>,
        check: impl Fn() -> Result<()>,
    ) -> Result<SshLaunchRequest> {
        check()?;
        let profile = profile.clone().normalize()?;
        if profile
            .connection_options
            .as_ref()
            .is_some_and(|o| o.unsupported_proxy.is_some())
        {
            return Err(Error::UnsupportedProxy);
        }
        if profile.user.is_empty() {
            return Err(Error::Validation);
        }
        if let Some(file) = &profile.key_file {
            self.directory.check_identity(Path::new(file))?;
        }
        let pty = RemotePty::new(pty.cols, pty.rows)?;
        let mut scan_args: Vec<OsString> = vec![
            "-T".into(),
            "10".into(),
            "-p".into(),
            profile.port.to_string().into(),
            "-t".into(),
            "ed25519,ecdsa,rsa".into(),
        ];
        let family = profile
            .connection_options
            .as_ref()
            .and_then(|o| o.address_family)
            .unwrap_or(0);
        if family == 4 {
            scan_args.push("-4".into());
        } else if family == 6 {
            scan_args.push("-6".into());
        }
        scan_args.extend(["--".into(), profile.host.clone().into()]);
        let output = self.runner.run(&ProcessRequest {
            executable: self.programs.keyscan.clone(),
            args: scan_args,
            timeout: Duration::from_secs(15),
            max_output: 256 * 1024,
        })?;
        if !output.success {
            return Err(Error::Process);
        }
        let key = scanned_key(&output.stdout)?;
        self.known
            .verify_guarded(&profile.host, profile.port, &key, confirm, &check)?;
        check()?;
        let alias = format!(
            "shellfox-pin-{}",
            URL_SAFE_NO_PAD.encode(Sha256::digest(&key))
        );
        let pin_name = format!("{alias}.hosts");
        let pin = format!(
            "{alias} {} {}\n",
            host_key_info(&key)?.key_type,
            STANDARD.encode(&key)
        );
        self.directory
            .atomic_write(&pin_name, pin.as_bytes(), 256 * 1024)?;
        // Both files are application-owned. OpenSSH must never consult the user's config
        // or global trust, and must never update these files on its own.
        let empty = "ssh-empty.config";
        self.directory.atomic_write(empty, b"", 1)?;
        let empty_path = config_path(&self.directory.file_path(empty)?)?;
        let pin_path = config_path(&self.directory.file_path(&pin_name)?)?;
        let mut args: Vec<OsString> = vec![
            "-F".into(),
            self.directory.file_path(empty)?.into_os_string(),
            "-tt".into(),
            "-p".into(),
            profile.port.to_string().into(),
            "-l".into(),
            profile.user.clone().into(),
        ];
        let key_type = host_key_info(&key)?.key_type;
        let algorithms = if key_type == "ssh-rsa" {
            "rsa-sha2-512,rsa-sha2-256"
        } else {
            &key_type
        };
        for option in [
            "StrictHostKeyChecking=yes".to_owned(),
            format!("HostKeyAlias={alias}"),
            format!("UserKnownHostsFile={pin_path}"),
            format!("GlobalKnownHostsFile={empty_path}"),
            format!("HostKeyAlgorithms={algorithms}"),
            "KnownHostsCommand=none".into(),
            "VerifyHostKeyDNS=no".into(),
            "UpdateHostKeys=no".into(),
            "CheckHostIP=no".into(),
            "ProxyCommand=none".into(),
            "ProxyJump=none".into(),
            "PermitLocalCommand=no".into(),
            "ClearAllForwardings=yes".into(),
            "ForwardX11=no".into(),
            "IdentityFile=none".into(),
            "TCPKeepAlive=yes".into(),
            "ServerAliveInterval=30".into(),
            "ServerAliveCountMax=2147483647".into(),
            "ConnectTimeout=120".into(),
        ] {
            args.extend(["-o".into(), option.into()]);
        }
        if family == 4 {
            args.push("-4".into());
        } else if family == 6 {
            args.push("-6".into());
        }
        let options = profile.connection_options.as_ref();
        args.push(
            if options.and_then(|o| o.agent_forward).unwrap_or(false) {
                "-A"
            } else {
                "-a"
            }
            .into(),
        );
        if options.and_then(|o| o.try_agent) == Some(false) {
            args.extend(["-o".into(), "IdentityAgent=none".into()]);
        }
        if let Some(file) = &profile.key_file {
            args.extend(["-i".into(), file.into()]);
        }
        let environment = remote_environment(env);
        let set_env = environment
            .iter()
            .filter(|(name, _)| *name != "TERM")
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(" ");
        args.extend([
            "-o".into(),
            format!("SetEnv={set_env}").into(),
            "--".into(),
            profile.host.into(),
        ]);
        let expected_authentication = AuthenticationProof::new();
        let command = profile
            .remote_cwd
            .as_deref()
            .map(remote_command)
            .unwrap_or_else(|| "exec \"${SHELL:-/bin/sh}\" -il".into());
        args.push(format!("{}{command}", expected_authentication.command()).into());
        Ok(SshLaunchRequest {
            executable: self.programs.ssh.clone(),
            args,
            environment,
            pty,
            authentication: HostAuthentication {
                saved_password: profile.password,
            },
            expected_authentication,
        })
    }
}
fn scanned_key(output: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(output).map_err(|_| Error::InvalidKey)?;
    let mut candidates = Vec::new();
    for line in text
        .lines()
        .take(128)
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3 {
            return Err(Error::InvalidKey);
        }
        let key = STANDARD.decode(fields[2]).map_err(|_| Error::InvalidKey)?;
        let info = host_key_info(&key)?;
        if info.key_type != fields[1] {
            return Err(Error::InvalidKey);
        }
        let rank = if info.key_type == "ssh-ed25519" {
            0
        } else if info.key_type.starts_with("ecdsa-") {
            1
        } else {
            2
        };
        candidates.push((rank, key));
    }
    candidates.sort_by_key(|(rank, _)| *rank);
    candidates
        .into_iter()
        .next()
        .map(|(_, key)| key)
        .ok_or(Error::InvalidKey)
}
fn config_path(path: &Path) -> Result<String> {
    let path = path.to_str().ok_or(Error::Validation)?.replace('\\', "/");
    if path
        .chars()
        .any(|c| c < ' ' || ['"', '%', '$'].contains(&c))
    {
        return Err(Error::Validation);
    }
    Ok(format!("\"{path}\""))
}
fn remote_command(cwd: &str) -> String {
    let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let directory = if cwd == "~" {
        "\"$HOME\"".into()
    } else if let Some(path) = cwd.strip_prefix("~/") {
        format!("\"$HOME\"/{}", quote(path))
    } else {
        quote(cwd)
    };
    format!("cd -- {directory} 2>/dev/null || printf '  remote folder unavailable; staying in home\\n' >&2; exec \"${{SHELL:-/bin/sh}}\" -il")
}
