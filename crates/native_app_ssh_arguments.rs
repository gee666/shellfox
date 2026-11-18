//! Explicit producer-pinned SSH roles; no personal profile/program discovery.
use anyhow::{Context, Result, bail, ensure};
use shellfox_service::native_providers::{NativeProgramResource, NativeSshPrograms, NativeSshResources};
use std::{ffi::OsString, path::{Component, Path, PathBuf}};

#[derive(Default)]
struct ProgramInput { path: Option<PathBuf>, digest: Option<[u8; 32]> }
impl ProgramInput {
    fn present(&self) -> bool { self.path.is_some() || self.digest.is_some() }
    fn push(&mut self, digest: bool, value: OsString) -> Result<()> {
        if digest {
            ensure!(self.digest.is_none(), "Duplicate SSH program digest");
            let value = value.to_str().context("SSH program digest must be UTF-8")?;
            ensure!(value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()), "SSH program digests require exactly 64 externally trusted hex digits");
            let mut bytes = [0; 32];
            for (index, byte) in bytes.iter_mut().enumerate() { *byte = u8::from_str_radix(&value[index*2..index*2+2], 16)?; }
            self.digest = Some(bytes);
        } else {
            ensure!(self.path.is_none(), "Duplicate SSH program path");
            let path = PathBuf::from(value);
            ensure!(path.is_absolute() && !path.components().any(|p| p == Component::ParentDir), "SSH programs require absolute paths without parent traversal");
            self.path = Some(path);
        }
        Ok(())
    }
    fn complete(self) -> Result<(PathBuf, [u8; 32])> {
        match (self.path, self.digest) {
            (Some(path), Some(digest)) => Ok((path, digest)),
            _ => bail!("SSH requires all three explicit program paths and their trusted producer digests"),
        }
    }
}
#[derive(Default)]
pub(super) struct SshArguments { ssh: ProgramInput, keyscan: ProgramInput, askpass: ProgramInput }
impl SshArguments {
    pub fn is_empty(&self) -> bool { !self.ssh.present() && !self.keyscan.present() && !self.askpass.present() }
    pub fn is_flag(flag: &str) -> bool {
        matches!(flag, "--ssh-program" | "--ssh-program-sha256" | "--ssh-keyscan" | "--ssh-keyscan-sha256" | "--ssh-askpass-cli" | "--ssh-askpass-cli-sha256")
    }
    pub fn push(&mut self, flag: &str, value: OsString) -> Result<()> {
        ensure!(Self::is_flag(flag), "Unknown SSH resource flag");
        let digest = flag.ends_with("-sha256");
        match flag.trim_end_matches("-sha256") {
            "--ssh-program" => self.ssh.push(digest, value),
            "--ssh-keyscan" => self.keyscan.push(digest, value),
            "--ssh-askpass-cli" => self.askpass.push(digest, value),
            _ => bail!("Unknown SSH resource flag"),
        }
    }
    pub fn finish(self, helper: &Path, helper_digest: [u8; 32]) -> Result<Option<NativeSshResources>> {
        if self.is_empty() { return Ok(None); }
        // Complete all input validation before acquiring any file lease.
        let (ssh, ssh_digest) = self.ssh.complete()?;
        let (keyscan, keyscan_digest) = self.keyscan.complete()?;
        let (askpass, askpass_digest) = self.askpass.complete()?;
        let programs = NativeSshPrograms::new(
            NativeProgramResource::from_pinned_file(ssh, ssh_digest)?,
            NativeProgramResource::from_pinned_file(keyscan, keyscan_digest)?,
            NativeProgramResource::from_pinned_file(askpass, askpass_digest)?,
        );
        // Reuse the actual externally pinned Go helper on Windows, not a
        // discovered tool or a fresh self-hash. Unix uses its native policy.
        let permissions = if cfg!(windows) {
            Some(NativeProgramResource::from_pinned_file(helper.to_owned(), helper_digest)?)
        } else { None };
        Ok(Some(NativeSshResources::new(programs, permissions)?))
    }
}

impl super::ApplicationArguments {
    /// Retain the explicitly pinned program leases before bootstrap. No SSH
    /// store is materialized until Service acquires its original owner/actor.
    pub fn finish_with_helper(mut self, data: &Path, helper: &Path, helper_digest: [u8; 32]) -> Result<super::NativeApplicationConfig> {
        let ssh = std::mem::take(&mut self.ssh).finish(helper, helper_digest)?;
        let mut config = self.finish(data)?;
        if let Some(ssh) = ssh {
            let resources = config.provider_resources.take().unwrap_or_else(|| {
                super::ApplicationProviderResources::new(
                    std::sync::Arc::new(super::UnconfiguredInstallResources),
                    shellfox_service::system::SystemInstallPolicy::default(),
                )
            }).with_ssh(ssh);
            config = config.with_provider_resources(resources);
        }
        Ok(config)
    }
}

#[cfg(test)]
#[path = "native_app_ssh_arguments_tests.rs"]
mod tests;
