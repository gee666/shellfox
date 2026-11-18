use super::*;
#[test]
fn mapping_units_are_not_native_elevation_acceptance_and_never_default_uncertain_tokens_to_normal()
{
    let medium = WindowsLaunchToken {
        elevated: false,
        elevation_type: WindowsElevationType::Default,
        integrity_rid: 0x2000,
        ui_access: false,
    };
    assert_eq!(classify(medium), (LaunchPrivilege::Normal, None));
    assert_eq!(
        classify(WindowsLaunchToken {
            elevated: true,
            elevation_type: WindowsElevationType::Full,
            integrity_rid: 0x3000,
            ..medium
        }),
        (LaunchPrivilege::Elevated, None)
    );
    for token in [
        WindowsLaunchToken {
            ui_access: true,
            ..medium
        },
        WindowsLaunchToken {
            integrity_rid: 0x4000,
            ..medium
        },
        WindowsLaunchToken {
            elevation_type: WindowsElevationType::Full,
            ..medium
        },
    ] {
        assert_eq!(classify(token).0, LaunchPrivilege::Unknown);
    }
    assert_eq!(
        LaunchPrivilegeFacts::default().privilege,
        LaunchPrivilege::Unknown
    );
}
#[test]
fn binding_is_private_instance_correlation_not_a_reusable_enum_or_identifier() {
    let (binding, verifier) = new_launch_binding();
    let (other, other_verifier) = new_launch_binding();
    assert!(verifier.matches_binding(&binding));
    assert!(!verifier.matches_binding(&other));
    let evidence = NativeLaunchEvidence {
        seal: binding.0,
        facts: LaunchPrivilegeFacts::default(),
        #[cfg(windows)]
        _proof: None,
    };
    assert!(verifier.matches(&evidence));
    assert!(!other_verifier.matches(&evidence));
}
#[cfg(windows)]
#[test]
fn lack_of_exact_owned_job_proof_is_unknown_without_querying_an_arbitrary_pid() {
    let job = crate::windows::Job::new().unwrap();
    let (binding, verifier) = new_launch_binding();
    let evidence = capture_pinned_pty(
        binding,
        unsafe { windows_sys::Win32::System::Threading::GetCurrentProcess() },
        job.handle(),
    );
    assert!(verifier.matches(&evidence));
    assert_eq!(evidence.facts().privilege, LaunchPrivilege::Unknown);
    assert_eq!(evidence.facts().basis, LaunchEvidenceBasis::Unavailable);
    assert_eq!(
        evidence.facts().unknown,
        Some(LaunchUnknownReason::OwnershipUncertain)
    );
}
