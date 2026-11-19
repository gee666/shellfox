use super::*;
use anyhow::{ensure, Context, Result};
use std::{fs::File, os::fd::AsRawFd};

pub(crate) fn capture(
    binding: NativeLaunchBinding,
    pid: u32,
    pidfd: &File,
    mode: crate::UnixPrivilegeMode,
) -> Result<NativeLaunchEvidence> {
    let mut poll = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    ensure!(
        unsafe { libc::poll(&mut poll, 1, 0) } == 0,
        "original POSIX controller exited"
    );
    let status = std::fs::read_to_string(format!("/proc/{pid}/status"))?;
    let facts = parse(&status)?;
    ensure!(
        unsafe { libc::poll(&mut poll, 1, 0) } == 0,
        "original POSIX controller exited"
    );
    let current = unsafe { libc::getuid() };
    let certified = mode == crate::UnixPrivilegeMode::NonElevating
        && facts.is_non_elevating_account(current);
    ensure!(
        mode != crate::UnixPrivilegeMode::NonElevating || certified,
        "non-elevating POSIX launch did not establish same-account capability-free no_new_privs"
    );
    Ok(NativeLaunchEvidence {
        seal: binding.0,
        facts: LaunchPrivilegeFacts {
            privilege: if certified {
                LaunchPrivilege::Normal
            } else {
                LaunchPrivilege::Unknown
            },
            purpose: LaunchFactoryPurpose::OwnedPtyProgram,
            basis: LaunchEvidenceBasis::OriginalPinnedPosixCredentials,
            stage: LaunchEvidenceStage::BeforePublicPtyReady,
            token: None,
            posix: Some(facts),
            unknown: (!certified).then_some(LaunchUnknownReason::UnrestrictedPosixLaunch),
        },
    })
}
pub(crate) fn capture_raw(
    binding: NativeLaunchBinding,
    pid: u32,
    pidfd: &File,
    mode: crate::UnixPrivilegeMode,
) -> Result<NativeLaunchEvidence> {
    let mut evidence = capture(binding, pid, pidfd, mode)?;
    evidence.facts.purpose = LaunchFactoryPurpose::RawProgram;
    evidence.facts.stage = LaunchEvidenceStage::BeforeRawGoConfirmedReady;
    Ok(evidence)
}
fn parse(status: &str) -> Result<PosixLaunchFacts> {
    let field = |key: &str| -> Result<&str> {
        let mut values = status.lines().filter_map(|line| line.strip_prefix(key));
        let value = values.next().context("POSIX credential field missing")?;
        ensure!(values.next().is_none(), "duplicate POSIX credential field");
        Ok(value.trim())
    };
    let uid: Vec<u32> = field("Uid:")?
        .split_whitespace()
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    ensure!(uid.len() == 4, "invalid POSIX UID facts");
    let caps = |key| -> Result<u64> { Ok(u64::from_str_radix(field(key)?, 16)?) };
    let no_new_privs = match field("NoNewPrivs:")? {
        "0" => false,
        "1" => true,
        _ => anyhow::bail!("invalid no_new_privs facts"),
    };
    Ok(PosixLaunchFacts {
        uid: uid[0],
        euid: uid[1],
        saved_uid: uid[2],
        fs_uid: uid[3],
        effective_caps: caps("CapEff:")?,
        permitted_caps: caps("CapPrm:")?,
        inheritable_caps: caps("CapInh:")?,
        ambient_caps: caps("CapAmb:")?,
        no_new_privs,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_complete_unambiguous_kernel_facts() {
        let valid = "Uid:\t1000 1000 1000 1000\nCapEff:\t0\nCapPrm:\t0\nCapInh:\t0\nCapAmb:\t0\nNoNewPrivs:\t1\n";
        assert!(parse(valid).unwrap().no_new_privs);
        assert!(parse(&valid.replace("NoNewPrivs:\t1", "NoNewPrivs:\t2")).is_err());
        assert!(parse(&(valid.to_owned() + "Uid:\t0 0 0 0\n")).is_err());
        assert!(parse(&valid.replace("CapEff:\t0\n", "")).is_err());
    }
}
