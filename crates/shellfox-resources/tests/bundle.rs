#[path = "cases/aliases.rs"]
mod aliases;
#[path = "cases/framework.rs"]
mod framework;
#[path = "cases/lifetime.rs"]
mod lifetime;
#[path = "cases/producer.rs"]
mod producer;
mod support;
#[path = "cases/trusted_build.rs"]
mod trusted_build;
#[cfg(windows)]
#[path = "cases/windows.rs"]
mod windows;
use sha2::{Digest, Sha256};
use shellfox_resources::{
    Error, ExpectedManifestDigest, ProtocolId, VerifiedBundle, MANIFEST_NAME,
};
use std::fs;
use support::Fixture;

#[test]
fn complete_publish_is_verified_against_an_external_anchor() {
    let fixture = Fixture::new();
    let bundle = fixture.open().unwrap();
    assert_eq!(bundle.identity(), &fixture.manifest.identity);
    assert_eq!(bundle.file_set(), &fixture.manifest.files);
    assert_eq!(bundle.expected_manifest_digest(), fixture.anchor);
    bundle.revalidate().unwrap();
    assert_eq!(
        bundle.read_verified("nested/lazy.dll", 1024).unwrap(),
        b"synthetic nested payload"
    );
}

#[test]
fn modified_managed_dll_is_rejected_even_when_the_apphost_is_unchanged() {
    let fixture = Fixture::new();
    let apphost = fs::read(fixture.root.join("Shellfox.ElevatedTerminal.exe")).unwrap();
    fs::write(
        fixture.root.join("Shellfox.ElevatedTerminal.dll"),
        b"different managed application",
    )
    .unwrap();
    assert_eq!(
        fs::read(fixture.root.join("Shellfox.ElevatedTerminal.exe")).unwrap(),
        apphost
    );
    assert!(fixture.open().is_err());
}

#[test]
fn replacing_manifest_and_hashes_cannot_replace_the_external_trust_anchor() {
    let mut fixture = Fixture::new();
    let trusted_anchor = fixture.anchor;
    fs::write(
        fixture.root.join("Shellfox.ElevatedTerminal.dll"),
        b"tampered assembly",
    )
    .unwrap();
    fixture.refresh_files();
    assert!(matches!(
        VerifiedBundle::open(&fixture.root, trusted_anchor, &fixture.manifest.identity),
        Err(Error::ManifestDigest)
    ));
}

#[test]
fn missing_runtime_and_unlisted_dlls_fail_closed() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.root.join("coreclr.dll")).unwrap();
    assert!(fixture.open().is_err());
    let mut fixture = Fixture::new();
    fs::remove_file(fixture.root.join("hostpolicy.dll")).unwrap();
    fixture.refresh_files();
    assert!(
        fixture.open().is_err(),
        "a newly trusted manifest still cannot omit mandatory runtime files"
    );
    let fixture = Fixture::new();
    fs::write(fixture.root.join("extra.dll"), b"unlisted").unwrap();
    assert!(fixture.open().is_err());
}

#[test]
fn traversal_case_aliases_and_reserved_names_are_rejected() {
    for relative in [
        "../escape.dll",
        "/absolute.dll",
        "nested\\escape.dll",
        "con.dll",
        "a/../b.dll",
        "a//b.dll",
        "C:stream.dll",
        "trailing.",
    ] {
        let mut fixture = Fixture::new();
        fixture.manifest.files.0[0].path = relative.into();
        fixture.write_manifest();
        assert!(fixture.open().is_err(), "accepted {relative}");
    }
    let mut fixture = Fixture::new();
    let mut duplicate = fixture.manifest.files.0[0].clone();
    duplicate.path = duplicate.path.to_ascii_uppercase();
    fixture.manifest.files.0.push(duplicate);
    fixture.write_manifest();
    assert!(fixture.open().is_err());
}

#[test]
fn strict_schema_identity_rid_and_capabilities_are_bound() {
    let fixture = Fixture::new();
    let mut expected = fixture.manifest.identity.clone();
    expected.version.push_str("-other");
    assert!(matches!(
        VerifiedBundle::open(&fixture.root, fixture.anchor, &expected),
        Err(Error::Identity)
    ));
    for mutation in [0, 1, 2] {
        let mut fixture = Fixture::new();
        match mutation {
            0 => fixture.manifest.entrypoints[0].protocols[0].version = 2,
            1 => {
                fixture.manifest.entrypoints[0].protocols[0]
                    .capabilities
                    .pop();
            }
            _ => fixture.manifest.format = "future-format".into(),
        }
        fixture.write_manifest();
        assert!(fixture.open().is_err());
    }
    let fixture = Fixture::new();
    let mut json = serde_json::to_value(&fixture.manifest).unwrap();
    json["unrecognized"] = true.into();
    let bytes = serde_json::to_vec(&json).unwrap();
    fs::write(fixture.root.join(MANIFEST_NAME), &bytes).unwrap();
    let anchor = ExpectedManifestDigest::from_trusted_bytes(Sha256::digest(bytes).into());
    assert!(VerifiedBundle::open(&fixture.root, anchor, &fixture.manifest.identity).is_err());
}

#[test]
fn dependency_rid_runtime_version_and_referenced_assets_are_checked() {
    for body in [
        r#"{"runtimeTarget":{"name":".NETCoreApp,Version=v8.0/win-arm64"}}"#,
        r#"{"runtimeTarget":{"name":".NETCoreApp,Version=v8.0/win-x64"},"targets":{".NETCoreApp,Version=v8.0/win-x64":{"runtimepack.Microsoft.NETCore.App.Runtime.win-x64/8.0.31":{"runtime":{"missing.dll":{}}}}}}"#,
    ] {
        let mut fixture = Fixture::new();
        fs::write(
            fixture.root.join("Shellfox.ElevatedTerminal.deps.json"),
            body,
        )
        .unwrap();
        fixture.refresh_files();
        assert!(fixture.open().is_err());
    }
    let mut fixture = Fixture::new();
    fs::write(fixture.root.join("Shellfox.ElevatedTerminal.runtimeconfig.json"), r#"{"runtimeOptions":{"tfm":"net8.0","includedFrameworks":[{"name":"Microsoft.NETCore.App","version":"8.0.99"}]}}"#).unwrap();
    fixture.refresh_files();
    assert!(fixture.open().is_err());
}

#[test]
fn development_probing_configuration_is_not_a_closed_publish() {
    let mut fixture = Fixture::new();
    fs::write(
        fixture
            .root
            .join("Shellfox.ElevatedTerminal.runtimeconfig.dev.json"),
        b"{}",
    )
    .unwrap();
    fixture.refresh_files();
    assert!(fixture.open().is_err());
}

#[test]
fn hardlinks_cannot_alias_a_trusted_payload() {
    let fixture = Fixture::new();
    let alias = fixture.root.join("alias.dll");
    fs::hard_link(fixture.root.join("Shellfox.ElevatedTerminal.dll"), &alias).unwrap();
    assert!(fixture.open().is_err());
}

#[test]
fn resource_reads_are_bounded_and_cannot_read_unlisted_files() {
    let fixture = Fixture::new();
    let bundle = fixture.open().unwrap();
    assert!(bundle.read_verified("nested/lazy.dll", 2).is_err());
    assert!(bundle.read_verified("../outside", 1024).is_err());
    assert!(bundle.read_verified(MANIFEST_NAME, 1024).is_err());
    assert!(bundle
        .executable("nested/lazy.dll", ProtocolId::PowerGuardian)
        .is_err());
    assert!(ExpectedManifestDigest::from_trusted_hex(&"A".repeat(64)).is_err());
}

#[cfg(windows)]
#[test]
fn original_handles_deny_dll_config_writes_deletes_and_replacement_until_last_owner_drops() {
    let fixture = Fixture::new();
    let bundle = fixture.open().unwrap();
    let executable = fixture.executable(&bundle);
    drop(bundle);
    for relative in [
        "Shellfox.ElevatedTerminal.dll",
        "Shellfox.ElevatedTerminal.deps.json",
        "Shellfox.ElevatedTerminal.runtimeconfig.json",
        "coreclr.dll",
        "nested/lazy.dll",
        MANIFEST_NAME,
    ] {
        let file = fixture.root.join(relative);
        assert!(fs::write(&file, b"replacement").is_err());
        assert!(fs::remove_file(&file).is_err());
        assert!(fs::rename(&file, file.with_extension("replaced")).is_err());
    }
    drop(executable);
    fs::write(
        fixture.root.join("coreclr.dll"),
        b"write allowed after confirmed release",
    )
    .unwrap();
}

#[cfg(windows)]
#[test]
fn a_file_added_between_command_check_and_spawn_is_rejected_without_launch() {
    let fixture = Fixture::new();
    let bundle = fixture.open().unwrap();
    let command = fixture.executable(&bundle).command().unwrap();
    // Directory handles prevent rename, not insertion. The final exact-set
    // check is required, and is not advertised as a namespace write lock.
    fs::write(fixture.root.join("late.dll"), b"unlisted late payload").unwrap();
    assert!(matches!(command.spawn(), Err(Error::Changed(_))));
}

#[cfg(unix)]
#[test]
fn unix_symlinks_are_rejected_and_path_execution_is_not_falsely_immutable() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    fs::remove_file(fixture.root.join("nested/lazy.dll")).unwrap();
    symlink(
        fixture.root.join("coreclr.dll"),
        fixture.root.join("nested/lazy.dll"),
    )
    .unwrap();
    assert!(fixture.open().is_err());
    let fixture = Fixture::new();
    let bundle = fixture.open().unwrap();
    assert!(matches!(
        fixture.executable(&bundle).path_for_spawn(),
        Err(Error::ImmutableExecutionUnavailable)
    ));
    fs::write(
        fixture.root.join("coreclr.dll"),
        b"Unix read fd is not a write-denial lock",
    )
    .unwrap();
    assert!(bundle.revalidate().is_err());
}
