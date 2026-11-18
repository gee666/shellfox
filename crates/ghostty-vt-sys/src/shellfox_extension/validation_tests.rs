use super::*;

// These constructed IDs/receiver tokens exercise POLICY ONLY. No DLL is
// verified, no body is decoded, and no cross-platform compatibility is proved.
fn policy_metadata(sender: EngineIdentity) -> SnapshotMetadata {
    SnapshotMetadata {
        engine_identity: sender,
        wire_profile: 1,
        library_version: 0x00010001,
        snapshot_version: 1,
        state: Some(ProfileState {
            abi_version: 1,
            terminal_profile: TerminalProfile::Native,
            preceding_join: 0,
        }),
        configuration: RestorationConfig {
            runtime_image_storage_limit: 1234,
            runtime_resize_pull_scrollback: false,
        },
    }
}
#[test]
fn cross_target_native0_identity_admission_is_explicit_policy_only() {
    let receiver = VerifiedNativeExtension {
        identity: EngineIdentity::from_hex(&"1".repeat(64)).unwrap(),
    };
    let sender = EngineIdentity::from_hex(&"2".repeat(64)).unwrap();
    let metadata = policy_metadata(sender);
    let original = metadata.clone();
    let compatibility = Native0RawV1Compatibility::for_negotiated_sender(receiver, sender);
    assert_eq!(metadata.validate(receiver), Err(Error::IdentityMismatch));
    assert_eq!(
        compatibility.validate(receiver, &metadata),
        metadata.validate_native0_raw_v1()
    );
    let wrong = Native0RawV1Compatibility::for_negotiated_sender(receiver, receiver.identity);
    assert_eq!(
        wrong.validate(receiver, &metadata),
        Err(Error::IdentityMismatch)
    );
    let other_receiver = VerifiedNativeExtension { identity: sender };
    assert_eq!(
        compatibility.validate(other_receiver, &metadata),
        Err(Error::IdentityMismatch)
    );
    assert_eq!(metadata, original);
    for pull in [false, true] {
        let configuration = RestorationConfig {
            runtime_resize_pull_scrollback: pull,
            ..metadata.configuration
        };
        let options = DecodeOptions::build(&receiver, SnapshotLimits::default(), configuration);
        assert_eq!(
            options.engine_profile.ptr,
            receiver.identity.as_bytes().as_ptr()
        );
        assert_eq!(options.engine_profile.len, 64);
        assert_eq!(options.runtime_resize_pull_scrollback, u32::from(pull));
        assert_eq!(options.runtime_image_storage_limit, 1234);
        assert_eq!(options.runtime_image_io_policy, 0);
    }
}
#[test]
fn cross_target_native0_rejects_other_profiles_versions_and_invalid_state_policy_only() {
    let receiver = VerifiedNativeExtension {
        identity: EngineIdentity::from_hex(&"1".repeat(64)).unwrap(),
    };
    let sender = EngineIdentity::from_hex(&"2".repeat(64)).unwrap();
    let compatibility = Native0RawV1Compatibility::for_negotiated_sender(receiver, sender);
    let original = policy_metadata(sender);
    for mutation in 0..7 {
        let mut metadata = original.clone();
        match mutation {
            0 => metadata.wire_profile = 2,
            1 => metadata.library_version = 0x00010002,
            2 => metadata.snapshot_version = 2,
            3 => metadata.state = None,
            4 => metadata.state.as_mut().unwrap().abi_version = 2,
            5 => metadata.state.as_mut().unwrap().terminal_profile = TerminalProfile::LegacyXterm6,
            _ => metadata.state.as_mut().unwrap().preceding_join = 1,
        }
        assert!(compatibility.validate(receiver, &metadata).is_err());
    }
    #[cfg(target_pointer_width = "32")]
    {
        let mut metadata = original;
        metadata.configuration.runtime_image_storage_limit = u64::MAX;
        assert!(compatibility.validate(receiver, &metadata).is_err());
    }
}

#[test]
fn malformed_native_state_is_never_a_successful_public_state() {
    let mut value = RawState::native();
    value.size -= 1;
    assert!(ProfileState::checked(value).is_err());
    value = RawState::native();
    value.abi_version = 2;
    assert!(ProfileState::checked(value).is_err());
    value = RawState::native();
    value.preceding_join = 2;
    assert!(ProfileState::checked(value).is_err());
    value = RawState::native();
    value.terminal_profile = 1;
    assert_eq!(ProfileState::checked(value), Err(Error::UnsupportedLegacy));
    value.terminal_profile = u32::MAX;
    assert!(matches!(
        ProfileState::checked(value),
        Err(Error::InvalidOutput(_))
    ));
}

#[test]
fn malformed_budget_reports_are_rejected_before_publication() {
    let limits = SnapshotLimits::default();
    let mut value = RawReport::new();
    value.source_offset = 20;
    assert!(SnapshotReport::checked(value, limits, 20, false, true).is_ok());
    for mutation in 0..7 {
        let mut bad = value;
        match mutation {
            0 => bad.size = 0,
            1 => bad.abi_version = 2,
            2 => bad.reserved = 1,
            3 => bad.heap_bytes = u64::MAX,
            4 => bad.work_units = limits.max_work_units + 1,
            5 => bad.source_offset = 19,
            _ => bad.source_offset = 21,
        }
        assert!(SnapshotReport::checked(bad, limits, 20, false, true).is_err());
    }
    assert!(SnapshotReport::checked(value, limits, 20, true, true).is_err());
}

#[test]
fn finite_zero_budget_is_not_unlimited_and_absurd_limits_are_rejected() {
    let zero = SnapshotLimits {
        max_allocation_bytes: 0,
        ..Default::default()
    };
    assert!(zero.validate().is_ok());
    let absurd = SnapshotLimits {
        max_encoded_bytes: usize::MAX,
        ..Default::default()
    };
    assert!(absurd.validate().is_err());
}
