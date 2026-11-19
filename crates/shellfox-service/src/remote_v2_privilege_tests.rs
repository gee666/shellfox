//! Policy checks on descriptive values only, not a native evidence factory.
use super::*;
fn posix() -> LaunchPrivilegeFacts {
    LaunchPrivilegeFacts {
        privilege: LaunchPrivilege::Normal,
        purpose: LaunchFactoryPurpose::OwnedPtyProgram,
        basis: LaunchEvidenceBasis::OriginalPinnedPosixCredentials,
        stage: LaunchEvidenceStage::BeforePublicPtyReady,
        token: None,
        posix: Some(shellfox_runtime::PosixLaunchFacts {
            uid: 1000,
            euid: 1000,
            saved_uid: 1000,
            fs_uid: 1000,
            effective_caps: 0,
            permitted_caps: 0,
            inheritable_caps: 0,
            ambient_caps: 0,
            no_new_privs: true,
        }),
        unknown: None,
    }
}
#[test]
fn posix_policy_requires_exact_same_nonroot_ids_all_caps_zero_and_nnp() {
    let facts = posix();
    assert!(exact_posix_normal(&facts, 1000));
    assert!(!exact_posix_normal(&facts, 0));
    assert!(!exact_posix_normal(&facts, 1001));
    let mutations: [fn(&mut shellfox_runtime::PosixLaunchFacts); 9] = [
        |p| p.uid = 0,
        |p| p.euid = 0,
        |p| p.saved_uid = 0,
        |p| p.fs_uid = 0,
        |p| p.effective_caps = 1,
        |p| p.permitted_caps = 1,
        |p| p.inheritable_caps = 1,
        |p| p.ambient_caps = 1,
        |p| p.no_new_privs = false,
    ];
    for mutate in mutations {
        let mut facts = posix();
        mutate(facts.posix.as_mut().expect("descriptive fixture"));
        assert!(!exact_posix_normal(&facts, 1000));
    }
}
#[test]
fn unrestricted_local_even_with_inherited_nnp_is_never_relabelled_normal() {
    let mut facts = posix();
    facts.privilege = LaunchPrivilege::Unknown;
    facts.unknown = Some(LaunchUnknownReason::UnrestrictedPosixLaunch);
    assert!(!exact_posix_normal(&facts, 1000));
    assert!(!accepted_launch(&facts));
    assert!(local_only_reason(&facts).unwrap().contains("local-only"));
}
#[test]
fn posix_policy_rejects_wrong_factory_stage_missing_or_mixed_evidence() {
    let mut facts = posix();
    facts.purpose = LaunchFactoryPurpose::StandardRawCreateProcess;
    assert!(!exact_posix_normal(&facts, 1000));
    facts = posix();
    facts.stage = LaunchEvidenceStage::NotCaptured;
    assert!(!exact_posix_normal(&facts, 1000));
    facts = posix();
    facts.posix = None;
    assert!(!exact_posix_normal(&facts, 1000));
    facts = posix();
    facts.unknown = Some(LaunchUnknownReason::OwnershipUncertain);
    assert!(!exact_posix_normal(&facts, 1000));
    facts = posix();
    facts.basis = LaunchEvidenceBasis::OriginalPinnedChildToken;
    assert!(!exact_posix_normal(&facts, 1000));
    facts = posix();
    facts.token = Some(shellfox_runtime::WindowsLaunchToken {
        elevated: false,
        elevation_type: shellfox_runtime::WindowsElevationType::Limited,
        integrity_rid: 0x2000,
        ui_access: false,
    });
    assert!(!exact_posix_normal(&facts, 1000));
}
#[test]
fn darwin_policy_requires_the_same_nonroot_original_pty_account() {
    let mut facts = LaunchPrivilegeFacts {
        basis: LaunchEvidenceBasis::OriginalDarwinCredentials { uid: 501 },
        posix: None,
        ..posix()
    };
    assert!(exact_darwin_normal(&facts, 501));
    assert!(!exact_darwin_normal(&facts, 502));
    assert!(!exact_darwin_normal(&facts, 0));
    facts.stage = LaunchEvidenceStage::BeforeRawGoConfirmedReady;
    assert!(!exact_darwin_normal(&facts, 501));
    facts.stage = LaunchEvidenceStage::BeforePublicPtyReady;
    facts.posix = posix().posix;
    assert!(!exact_darwin_normal(&facts, 501));
    facts.posix = None;
    facts.basis = LaunchEvidenceBasis::OriginalDarwinCredentials { uid: 0 };
    assert!(!exact_darwin_normal(&facts, 0));
}
#[test]
fn elevated_and_unavailable_facts_stay_local_only() {
    let mut facts = posix();
    facts.privilege = LaunchPrivilege::Elevated;
    assert!(!accepted_launch(&facts));
    assert!(local_only_reason(&facts).unwrap().contains("Elevated"));
    assert!(!accepted_launch(&LaunchPrivilegeFacts::default()));
}
