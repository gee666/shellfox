use super::support::Fixture;
use sha2::{Digest, Sha256};
use shellfox_resources::{
    expected_capabilities, BundleFile, BundleIdentity, BundleManifest, EntryPoint,
    ExpectedManifestDigest, FileSet, ProtocolContract, ProtocolId, ResourceRuntime, RuntimeId,
    TrustedBundleId, VerifiedBundle, MANIFEST_NAME,
};
use std::{path::Path, sync::Arc};

/// A synthetic trusted build fixture. Its EXE is the owned Rust fake helper,
/// never an installed managed app. No NoDynamicCodeLoad grant is constructed.
pub fn prepare(fixture: &Fixture) -> Arc<VerifiedBundle> {
    for name in [
        "Shellfox.ElevatedTerminal.dll",
        "hostfxr.dll",
        "hostpolicy.dll",
        "coreclr.dll",
        "clrjit.dll",
        "System.Private.CoreLib.dll",
        "lazy.dll",
    ] {
        std::fs::write(
            fixture.root.join(name),
            format!("owned synthetic asset {name}"),
        )
        .unwrap();
    }
    let rid = if cfg!(target_arch = "aarch64") {
        RuntimeId::WinArm64
    } else {
        RuntimeId::WinX64
    };
    let target = format!(".NETCoreApp,Version=v10.0/{}", rid.as_str());
    let runtime = format!(
        "runtimepack.Microsoft.NETCore.App.Runtime.{}/10.0.15",
        rid.as_str()
    );
    let config = serde_json::json!({"runtimeOptions":{"tfm":"net10.0","includedFrameworks":[{"name":"Microsoft.NETCore.App","version":"10.0.15"}]}});
    let deps = serde_json::json!({"runtimeTarget":{"name":target},"targets":{(target):{(runtime):{"runtime":{"System.Private.CoreLib.dll":{}},"native":{"coreclr.dll":{}}}}}});
    std::fs::write(
        fixture
            .root
            .join("Shellfox.ElevatedTerminal.runtimeconfig.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    std::fs::write(
        fixture.root.join("Shellfox.ElevatedTerminal.deps.json"),
        serde_json::to_vec(&deps).unwrap(),
    )
    .unwrap();
    let identity = BundleIdentity {
        id: TrustedBundleId::WindowsManagedHelper,
        rid,
        version: "0.1.0-owned-fixture".into(),
    };
    let manifest = BundleManifest {
        format: "shellfox-native-bundle-v1".into(),
        identity: identity.clone(),
        runtime: ResourceRuntime::Dotnet {
            version: "10.0.15".into(),
            target_framework: "net10.0-windows".into(),
            entry_assembly: "Shellfox.ElevatedTerminal.dll".into(),
            self_contained: true,
        },
        entrypoints: vec![EntryPoint {
            path: "Shellfox.ElevatedTerminal.exe".into(),
            protocols: [
                ProtocolId::ElevatedConpty,
                ProtocolId::ElevatedIsolated,
                ProtocolId::PowerGuardian,
            ]
            .into_iter()
            .map(|id| ProtocolContract {
                id,
                version: 1,
                capabilities: expected_capabilities(id, rid).into_iter().collect(),
            })
            .collect(),
        }],
        files: FileSet(files(&fixture.root)),
    };
    let bytes = serde_json::to_vec(&manifest).unwrap();
    std::fs::write(fixture.root.join(MANIFEST_NAME), &bytes).unwrap();
    let expected = ExpectedManifestDigest::from_trusted_bytes(Sha256::digest(&bytes).into());
    VerifiedBundle::open(&fixture.root, expected, &identity).unwrap()
}
fn files(root: &Path) -> Vec<BundleFile> {
    let mut files: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let bytes = std::fs::read(entry.path()).unwrap();
            BundleFile {
                path: entry.file_name().into_string().unwrap(),
                size: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
            }
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}
