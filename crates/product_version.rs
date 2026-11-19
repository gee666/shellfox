pub fn export() -> String {
    use std::{env, path::PathBuf, process::Command};
    let package =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"))
            .join("../../package.json");
    println!("cargo:rerun-if-changed={}", package.display());
    println!("cargo:rerun-if-env-changed=NODE_BINARY");
    let node = env::var_os("NODE_BINARY").unwrap_or_else(|| "node".into());
    let output = Command::new(node)
        .arg("-e")
        .arg("process.stdout.write(require(process.argv[1]).version)")
        .arg(package)
        .output()
        .expect("Node.js is required to read canonical Shellfox build metadata");
    assert!(output.status.success(), "read Shellfox product version");
    let version = String::from_utf8(output.stdout).expect("UTF-8 product version");
    assert!(
        version.split('.').count() == 3
            && version.split('.').all(|part| part
                .parse::<u16>()
                .is_ok_and(|value| value.to_string() == part)),
        "invalid canonical product version"
    );
    println!("cargo:rustc-env=SHELLFOX_VERSION={version}");
    version
}
