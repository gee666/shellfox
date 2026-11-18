use super::support::Fixture;
use shellfox_resources::{ExpectedManifestDigest, VerifiedBundle, MANIFEST_NAME};
use std::{fs, path::PathBuf, process::Command};

struct RecordCleanup(PathBuf);
impl Drop for RecordCleanup {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_file(&self.0).unwrap();
        }
    }
}

#[test]
#[ignore = "requires Node; run explicitly with producer::node_manifest_matches_the_rust_contract --ignored"]
fn node_manifest_matches_the_rust_contract() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.root.join(MANIFEST_NAME)).unwrap();
    let record = RecordCleanup(fixture.root.with_extension("trusted-build-record.json"));
    let project = fixture.root.parent().unwrap().parent().unwrap();
    let mut command = Command::new("node");
    command
        .current_dir(project)
        .args([
            "scripts/rust-package-resource-manifest.mjs",
            "managed",
            "--trusted-build-output",
        ])
        .arg(&fixture.root)
        .args(["--provenance"])
        .arg(&record.0)
        .args([
            "--rid",
            "win-x64",
            "--version",
            &fixture.manifest.identity.version,
            "--runtime-version",
            "8.0.31",
        ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "trusted producer: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let build: serde_json::Value = serde_json::from_slice(&fs::read(&record.0).unwrap()).unwrap();
    let anchor =
        ExpectedManifestDigest::from_trusted_hex(build["expectedManifestDigest"].as_str().unwrap())
            .unwrap();
    let bundle = VerifiedBundle::open(&fixture.root, anchor, &fixture.manifest.identity).unwrap();
    assert!(bundle
        .file_set()
        .0
        .iter()
        .any(|file| file.path == "managed-inventory.json"));
    assert!(bundle
        .file_set()
        .0
        .iter()
        .any(|file| file.path == "managed-vulnerability-report.json"));
    assert_eq!(build["safeProbeStatus"], "not-run");
    assert_eq!(build["managedAuditStatus"], "not-run");
}
