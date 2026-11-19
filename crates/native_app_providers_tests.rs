use super::*;
use shellfox_protocol::{AppError, ErrorCode};
use shellfox_service::{
    system::{CliInstallPlan, DesktopLaunch, SystemResult, SystemScope},
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct QueryFixture(Arc<AtomicUsize>);
fn unavailable<T>() -> SystemResult<T> {
    Err(AppError {
        code: ErrorCode::Unsupported,
        message: "Fixture has no stable installation layout".into(),
        retryable: false,
    })
}
impl SystemInstallResources for QueryFixture {
    fn cli_plan(&self, _: SystemScope) -> SystemResult<CliInstallPlan> {
        unavailable()
    }
    fn desktop_launch(&self) -> SystemResult<DesktopLaunch> {
        unavailable()
    }
    fn validate_current(&self) -> SystemResult<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        unavailable()
    }
}
#[test]
fn portable_manifest_external_anchor_and_temporary_installation_stay_fail_closed(
) -> anyhow::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    let fixture = tempfile::Builder::new()
        .prefix("portable-provider-input-")
        .tempdir_in(root)?;
    let payload = std::fs::canonicalize(fixture.path())?;
    #[cfg(windows)]
    let payload = std::path::PathBuf::from(
        payload
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("fixture path encoding"))?
            .strip_prefix(r"\\?\")
            .unwrap_or(payload.to_str().unwrap()),
    );
    std::fs::create_dir(payload.join("bin"))?;
    let platform = if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    };
    let cli = if cfg!(windows) {
        "bin/shellfox-cli.exe"
    } else {
        "bin/shellfox-cli"
    };
    let program = payload.join(cli);
    std::fs::write(&program, b"provider-program-fixture\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))?;
    }
    let bytes = format!("{{\"format\":\"shellfox-rust-portable-v1\",\"kind\":\"headless\",\"platform\":\"{platform}\",\"arch\":\"{arch}\",\"files\":[{{\"path\":\"{cli}\",\"sha256\":\"593e72c0b7aabeff5d43bc242b289b5602127a83a1f7404e74045ad61cc542c2\"}}],\"binaries\":[{{\"package\":\"shellfox-cli\",\"path\":\"{cli}\"}}]}}\n");
    let manifest = payload.join("package-manifest.json");
    std::fs::write(&manifest, bytes)?;
    // Digests published by the trusted fixture producer for these exact bytes,
    // not calculated from the runtime manifest to manufacture its authority.
    let hex = match (platform, arch) {
        ("win32", "x64") => "8aeaf87aee60fb2c6658fc1726dda1b12a24b3485f7e3d5cfcf034aa3a741ccf",
        ("win32", "arm64") => "3c0bbec350b914ffd1e3e5a149c3c5f58ad8bec8940c458900a7ef1fdf389ace",
        ("linux", "x64") => "b9f7b4576fbc8f78eb44cb826ba02c31b836941676c5279b86a63304165ea0d8",
        ("linux", "arm64") => "f980a8e9b88e39356364298987116093005a210dd5f3f5b2528b3331785d2a4c",
        ("darwin", "x64") => "9589e51d7cb8d40924114afb6f20dc4f4435e1ddc9e306c650003488e90bd1bf",
        ("darwin", "arm64") => "2cf867f978060ca3a91e06d10fb46c2fea22a1ee900990ed98a52847d87a7331",
        _ => anyhow::bail!("unsupported fixture target"),
    };
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)?;
    }
    let policy = SystemInstallPolicy {
        allow_user_install: true,
        allow_open: false,
    };
    assert!(
        ApplicationProviderResources::from_portable_manifest(&manifest, [0; 32], policy).is_err()
    );
    let temporary = ApplicationProviderResources::from_portable_manifest(&manifest, digest, policy)
        .unwrap_err();
    assert!(temporary.to_string().contains("Temporary"), "{temporary:#}");
    std::fs::write(&program, b"corrupt runtime payload")?;
    assert!(
        ApplicationProviderResources::from_portable_manifest(&manifest, digest, policy).is_err()
    );
    std::fs::write(&program, b"provider-program-fixture\n")?;
    std::fs::write(payload.join("unlisted.dll"), b"untrusted extra")?;
    assert!(
        ApplicationProviderResources::from_portable_manifest(&manifest, digest, policy).is_err()
    );
    Ok(())
}

#[test]
fn provider_builder_uses_explicit_policy_without_querying_or_installing() {
    let queries = Arc::new(AtomicUsize::new(0));
    let resources = ApplicationProviderResources::new(
        Arc::new(QueryFixture(queries.clone())),
        SystemInstallPolicy {
            allow_user_install: false,
            allow_open: true,
        },
    );
    let providers = resources.build(false);
    let cloned = providers.clone();
    assert!(Arc::ptr_eq(
        providers.system_backend(),
        cloned.system_backend()
    ));
    assert!(!providers.system_backend().policy.allow_user_install);
    assert!(providers.system_backend().policy.allow_open);
    assert_eq!(queries.load(Ordering::SeqCst), 0);
    let _same_backend_services = providers.system_services();
    assert_eq!(queries.load(Ordering::SeqCst), 0);
}
