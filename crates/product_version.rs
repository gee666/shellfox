/// Release version from package.json, checked and mapped by
/// scripts/release-version.mjs so every package format agrees.
pub struct ProductVersion {
    /// SemVer, such as 0.10.0 or 0.10.0-beta.1.
    pub version: String,
    /// Windows VERSIONINFO fields: major,minor,patch,build.
    #[allow(dead_code)]
    pub windows: String,
}

pub fn export() -> ProductVersion {
    use std::{env, path::PathBuf, process::Command};
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"))
        .join("../..");
    let package = root.join("package.json");
    let mapping = root.join("scripts/release-version.mjs");
    println!("cargo:rerun-if-changed={}", package.display());
    println!("cargo:rerun-if-changed={}", mapping.display());
    println!("cargo:rerun-if-env-changed=NODE_BINARY");
    let node = env::var_os("NODE_BINARY").unwrap_or_else(|| "node".into());
    let output = Command::new(node)
        .arg("-e")
        .arg(
            "import(require('node:url').pathToFileURL(process.argv[2]).href).then(({ releaseVersion }) => {
              const v = releaseVersion(require(process.argv[1]).version);
              process.stdout.write(`${v.version}\\n${v.core.replaceAll('.', ',')},${v.build}`);
            })",
        )
        .arg(package)
        .arg(mapping)
        .output()
        .expect("Node.js is required to read canonical Shellfox build metadata");
    assert!(
        output.status.success(),
        "invalid canonical product version: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("UTF-8 product version");
    let (version, windows) = text.split_once('\n').expect("mapped product version");
    println!("cargo:rustc-env=SHELLFOX_VERSION={version}");
    ProductVersion {
        version: version.to_owned(),
        windows: windows.to_owned(),
    }
}
