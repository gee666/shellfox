use sha2::{Digest, Sha256};
use shellfox_resources::{
    expected_capabilities, BundleFile, BundleIdentity, BundleManifest, EntryPoint,
    ExpectedManifestDigest, FileSet, ProtocolContract, ProtocolId, ResourceRuntime, RuntimeId,
    TrustedBundleId, VerifiedBundle, MANIFEST_NAME,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    pub root: PathBuf,
    pub manifest: BundleManifest,
    pub anchor: ExpectedManifestDigest,
}
impl Fixture {
    pub fn new() -> Self {
        let project = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("project root");
        let scratch = project.join("tmp");
        fs::create_dir_all(&scratch).unwrap();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = scratch.join(format!(
            "rust-package-resource fixture {}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        for name in [
            "Shellfox.ElevatedTerminal.exe",
            "Shellfox.ElevatedTerminal.dll",
            "hostfxr.dll",
            "hostpolicy.dll",
            "coreclr.dll",
            "clrjit.dll",
            "System.Private.CoreLib.dll",
        ] {
            fs::write(
                root.join(name),
                format!("synthetic fixture bytes for {name}"),
            )
            .unwrap();
        }
        fs::write(root.join("Shellfox.ElevatedTerminal.runtimeconfig.json"), br#"{"runtimeOptions":{"tfm":"net8.0","includedFrameworks":[{"name":"Microsoft.NETCore.App","version":"8.0.31"}]}}"#).unwrap();
        let deps = serde_json::json!({
            "runtimeTarget": { "name": ".NETCoreApp,Version=v8.0/win-x64" },
            "targets": { ".NETCoreApp,Version=v8.0/win-x64": {
                "runtimepack.Microsoft.NETCore.App.Runtime.win-x64/8.0.31": {
                    "runtime": { "System.Private.CoreLib.dll": {} }, "native": { "coreclr.dll": {} }
                }
            } },
            "libraries": { "runtimepack.Microsoft.NETCore.App.Runtime.win-x64/8.0.31": { "type": "runtimepack" },
                "Shellfox.ElevatedTerminal/1.0.0": { "type": "project" } }
        });
        fs::write(
            root.join("Shellfox.ElevatedTerminal.deps.json"),
            serde_json::to_vec(&deps).unwrap(),
        )
        .unwrap();
        fs::create_dir(root.join("nested")).unwrap();
        fs::write(root.join("nested/lazy.dll"), b"synthetic nested payload").unwrap();
        let rid = RuntimeId::WinX64;
        let manifest = BundleManifest {
            format: "shellfox-native-bundle-v1".into(),
            identity: BundleIdentity {
                id: TrustedBundleId::WindowsManagedHelper,
                rid,
                version: "0.1.0-fixture".into(),
            },
            runtime: ResourceRuntime::Dotnet {
                version: "8.0.31".into(),
                target_framework: "net8.0-windows".into(),
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
            files: FileSet(Vec::new()),
        };
        let mut fixture = Self {
            root,
            manifest,
            anchor: ExpectedManifestDigest::from_trusted_bytes([0; 32]),
        };
        fixture.refresh_files();
        fixture
    }
    pub fn set_runtime(&mut self, runtime: &str, rid: RuntimeId) {
        let framework = runtime.rsplit_once('.').expect("test runtime patch").0;
        self.manifest.identity.rid = rid;
        let ResourceRuntime::Dotnet {
            version,
            target_framework,
            ..
        } = &mut self.manifest.runtime
        else {
            panic!("managed fixture");
        };
        *version = runtime.into();
        *target_framework = format!("net{framework}-windows");
        let config = serde_json::json!({ "runtimeOptions": {
            "tfm": format!("net{framework}"), "includedFrameworks": [{ "name": "Microsoft.NETCore.App", "version": runtime }]
        }});
        let target = format!(".NETCoreApp,Version=v{framework}/{}", rid.as_str());
        let runtimepack = format!(
            "runtimepack.Microsoft.NETCore.App.Runtime.{}/{runtime}",
            rid.as_str()
        );
        let deps = serde_json::json!({
            "runtimeTarget": { "name": target },
            "targets": { (target): { (runtimepack.clone()): {
                "runtime": { "System.Private.CoreLib.dll": {} }, "native": { "coreclr.dll": {} }
            }}},
            "libraries": { (runtimepack): { "type": "runtimepack" }, "Shellfox.ElevatedTerminal/1.0.0": { "type": "project" } }
        });
        fs::write(
            self.root
                .join("Shellfox.ElevatedTerminal.runtimeconfig.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        fs::write(
            self.root.join("Shellfox.ElevatedTerminal.deps.json"),
            serde_json::to_vec(&deps).unwrap(),
        )
        .unwrap();
        self.refresh_files();
    }
    pub fn refresh_files(&mut self) {
        self.manifest.files = FileSet(walk(&self.root, &self.root));
        self.write_manifest();
    }
    pub fn write_manifest(&mut self) {
        let bytes = serde_json::to_vec(&self.manifest).unwrap();
        fs::write(self.root.join(MANIFEST_NAME), &bytes).unwrap();
        // A synthetic trusted BUILD fixture creates the anchor. Production
        // resource loading has no hash-the-supplied-manifest constructor.
        self.anchor = ExpectedManifestDigest::from_trusted_bytes(Sha256::digest(bytes).into());
    }
    pub fn open(&self) -> shellfox_resources::Result<Arc<VerifiedBundle>> {
        VerifiedBundle::open(&self.root, self.anchor, &self.manifest.identity)
    }
    pub fn executable(
        &self,
        bundle: &Arc<VerifiedBundle>,
    ) -> shellfox_resources::VerifiedExecutable {
        bundle
            .executable("Shellfox.ElevatedTerminal.exe", ProtocolId::PowerGuardian)
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("fixture handles must be released before cleanup");
    }
}
fn walk(root: &Path, directory: &Path) -> Vec<BundleFile> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == MANIFEST_NAME {
            continue;
        }
        if entry.file_type().unwrap().is_dir() {
            files.extend(walk(root, &entry.path()));
        } else {
            let bytes = fs::read(entry.path()).unwrap();
            files.push(BundleFile {
                path: entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/"),
                size: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}
