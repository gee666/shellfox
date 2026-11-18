use super::support::Fixture;
use std::fs::OpenOptions;

#[test]
fn a_preexisting_writer_prevents_verification_not_just_later_writes() {
    let fixture = Fixture::new();
    let writer = OpenOptions::new()
        .write(true)
        .open(fixture.root.join("coreclr.dll"))
        .unwrap();
    assert!(fixture.open().is_err());
    drop(writer);
    fixture.open().unwrap();
}

#[test]
fn managed_operations_do_not_infer_loader_policy_from_a_hash_or_probe() {
    let fixture = Fixture::new();
    let bundle = fixture.open().unwrap();
    let executable = fixture.executable(&bundle);
    assert!(matches!(
        executable.path_for_spawn(),
        Err(shellfox_resources::Error::LoaderPolicyRequired)
    ));
    let mut command = executable.command().unwrap();
    command.command_mut().arg("power-guardian");
    assert!(matches!(
        command.spawn(),
        Err(shellfox_resources::Error::LoaderPolicyRequired)
    ));
    let wrong = shellfox_resources::NoDynamicCodeLoad::from_trusted_review(
        shellfox_resources::ExpectedManifestDigest::from_trusted_bytes([0; 32]),
    );
    assert!(matches!(
        executable.command_with_loader_policy(&wrong),
        Err(shellfox_resources::Error::LoaderPolicyRequired)
    ));
    let mut command = executable.command().unwrap();
    command.command_mut().arg("probe-isolated");
    assert!(
        matches!(
            command.spawn(),
            Err(shellfox_resources::Error::LoaderPolicyRequired)
        ),
        "probe must match selected protocol"
    );
}

#[test]
fn a_verified_command_cannot_be_redirected_to_another_working_directory() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let bundle = fixture.open().unwrap();
    let mut command = fixture.executable(&bundle).command().unwrap();
    command.command_mut().current_dir(&outside.root);
    assert!(matches!(
        command.spawn(),
        Err(shellfox_resources::Error::Manifest(_))
    ));
}
