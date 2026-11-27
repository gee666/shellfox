use super::{InspectionPlatform as P, InspectionScope as S};

#[test]
fn a_wsl_root_is_inspected_on_windows() {
    assert!(S::Guest.inspectable());
    assert!(S::Guest.matches_platform(P::Windows));
    assert!(!S::Guest.matches_platform(P::Unix));
    assert!(S::LocalUnix.matches_platform(P::Unix));
    assert!(!S::Unsupported.inspectable() && !S::Ssh.inspectable());
}

#[test]
fn a_windows_tab_that_started_wsl_has_guest_processes() {
    use crate::activity::*;
    let row = |pid, is_root, executable: &str| OwnedProcess {
        identity: ProcessIdentity {
            domain: "windows/local".into(),
            pid,
            birth: pid.to_string(),
        },
        is_root,
        accessible: true,
        executable: Some(executable.into()),
        process_name: None,
        argv: None,
    };
    let snapshot = |processes| OwnedProcessSnapshot {
        identity: shellfox_runtime::Identity {
            session_id: Default::default(),
            tab_id: Default::default(),
            generation: Default::default(),
        },
        platform: ProcessPlatform::Windows,
        root: RootState::Alive,
        complete: true,
        processes,
    };
    let pwsh = || {
        row(
            1,
            true,
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
        )
    };
    let nested = |processes| super::GuestTarget::nested(&snapshot(processes));
    assert_eq!(nested(vec![pwsh()]), None);
    assert_eq!(
        nested(vec![pwsh(), row(2, false, r"C:\Windows\System32\WSL.EXE")]),
        Some(super::GuestTarget::Running)
    );
    // A WSL tab's own wsl.exe is its root, not a nested one.
    assert_eq!(
        nested(vec![row(1, true, r"C:\Windows\System32\wsl.exe")]),
        None
    );
}
