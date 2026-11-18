use shellfox_resources::{
    BundleIdentity, Error, ExpectedManifestDigest, ProtocolId, ResourceRuntime, RuntimeId,
    TrustedBundleId, VerifiedBundle,
};

#[test]
#[ignore = "requires an explicit trusted-build directory, expected digest, RID and version; verifies bytes only"]
fn full_managed_publish_matches_external_build_anchor() {
    let root = std::path::PathBuf::from(
        std::env::var_os("SHELLFOX_RESOURCE_TEST_BUILD_ROOT").expect("explicit trusted build root"),
    );
    let anchor = ExpectedManifestDigest::from_trusted_hex(
        &std::env::var("SHELLFOX_RESOURCE_TEST_BUILD_DIGEST")
            .expect("external trusted build digest"),
    )
    .unwrap();
    let rid = match std::env::var("SHELLFOX_RESOURCE_TEST_BUILD_RID")
        .unwrap()
        .as_str()
    {
        "win-x64" => RuntimeId::WinX64,
        "win-arm64" => RuntimeId::WinArm64,
        _ => panic!("expected Windows managed build RID"),
    };
    let identity = BundleIdentity {
        id: TrustedBundleId::WindowsManagedHelper,
        rid,
        version: std::env::var("SHELLFOX_RESOURCE_TEST_BUILD_VERSION")
            .expect("explicit build version"),
    };
    let bundle = VerifiedBundle::open(&root, anchor, &identity).unwrap();
    let ResourceRuntime::Dotnet {
        version,
        target_framework,
        ..
    } = &bundle.manifest().runtime
    else {
        panic!("managed build");
    };
    assert_eq!(version, "10.0.15");
    assert_eq!(target_framework, "net10.0-windows");
    assert_eq!(bundle.file_set().0.len(), 197);
    bundle.revalidate().unwrap();
    let executable = bundle
        .executable("Shellfox.ElevatedTerminal.exe", ProtocolId::PowerGuardian)
        .unwrap();
    #[cfg(windows)]
    assert!(matches!(
        executable.path_for_spawn(),
        Err(Error::LoaderPolicyRequired)
    ));
    #[cfg(unix)]
    assert!(matches!(
        executable.path_for_spawn(),
        Err(Error::ImmutableExecutionUnavailable)
    ));
    println!("Verified {} files, runtime {version}, RID {} against explicit external anchor; no execution or loader grant", bundle.file_set().0.len(), rid.as_str());
}
