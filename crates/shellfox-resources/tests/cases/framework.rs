use super::support::Fixture;
use shellfox_resources::{Error, ResourceRuntime, RuntimeId};
use std::fs;

#[test]
fn audited_frameworks_bind_declared_runtime_without_granting_loader_policy() {
    for runtime in ["8.0.31", "9.0.1", "10.0.15"] {
        for rid in [RuntimeId::WinX64, RuntimeId::WinArm64] {
            let mut fixture = Fixture::new();
            fixture.set_runtime(runtime, rid);
            let bundle = fixture.open().unwrap();
            assert_eq!(bundle.identity().rid, rid);
            let ResourceRuntime::Dotnet { version, .. } = &bundle.manifest().runtime else {
                panic!("managed fixture");
            };
            assert_eq!(version, runtime);
            #[cfg(windows)]
            assert!(matches!(
                fixture.executable(&bundle).path_for_spawn(),
                Err(Error::LoaderPolicyRequired)
            ));
            #[cfg(unix)]
            assert!(matches!(
                fixture.executable(&bundle).path_for_spawn(),
                Err(Error::ImmutableExecutionUnavailable)
            ));
        }
    }
}

#[test]
fn unsupported_or_noncanonical_framework_versions_are_rejected() {
    for runtime in [
        "7.0.20",
        "11.0.1",
        "10.1.0",
        "010.0.15",
        "10.0.015",
        "10.0.15-preview.1",
    ] {
        let mut fixture = Fixture::new();
        fixture.set_runtime(runtime, RuntimeId::WinX64);
        assert!(
            fixture.open().is_err(),
            "accepted unaudited version {runtime}"
        );
    }
}

#[test]
fn net10_manifest_cannot_use_net8_config_or_dependency_target() {
    for suffix in ["runtimeconfig.json", "deps.json"] {
        let mut fixture = Fixture::new();
        let file = fixture
            .root
            .join(format!("Shellfox.ElevatedTerminal.{suffix}"));
        let old = fs::read(&file).unwrap();
        fixture.set_runtime("10.0.15", RuntimeId::WinX64);
        fs::write(&file, old).unwrap();
        fixture.refresh_files();
        assert!(fixture.open().is_err(), "net10 accepted old {suffix}");
    }
    let mut fixture = Fixture::new();
    fixture.set_runtime("10.0.15", RuntimeId::WinX64);
    let ResourceRuntime::Dotnet {
        target_framework, ..
    } = &mut fixture.manifest.runtime
    else {
        panic!("managed fixture");
    };
    *target_framework = "net9.0-windows".into();
    fixture.write_manifest();
    assert!(fixture.open().is_err());
}

#[test]
fn net10_does_not_relax_missing_runtime_or_unlisted_payload_rejection() {
    for missing in [true, false] {
        let mut fixture = Fixture::new();
        fixture.set_runtime("10.0.15", RuntimeId::WinX64);
        if missing {
            fs::remove_file(fixture.root.join("coreclr.dll")).unwrap();
        } else {
            fs::write(fixture.root.join("unlisted.dll"), b"synthetic extra").unwrap();
        }
        assert!(fixture.open().is_err());
    }
}
