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
    // The desktop attaches only to a host process of the same build.
    println!("cargo:rustc-env=SHELLFOX_BUILD_ID={version}+{}", source_commit(&root));
    ProductVersion {
        version: version.to_owned(),
        windows: windows.to_owned(),
    }
}

/// The checked-out commit, marked when tracked files differ from it. Packages
/// built without a git checkout share an id per version.
fn source_commit(root: &std::path::Path) -> String {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let Some(commit) = git(&["rev-parse", "HEAD"]) else {
        return "unknown".into();
    };
    if let Some(directory) = git(&["rev-parse", "--absolute-git-dir"]) {
        for leaf in ["HEAD", "index"] {
            println!("cargo:rerun-if-changed={directory}/{leaf}");
        }
    }
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_none_or(|status| !status.is_empty());
    if dirty { format!("{commit}-dirty") } else { commit }
}
