fn main() {
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
        !version.is_empty()
            && version.len() <= 64
            && version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+')),
        "invalid canonical product version"
    );
    println!("cargo:rustc-env=SHELLFOX_PRODUCT_VERSION={version}");
    #[cfg(windows)]
    embed_windows_icon();
}

#[cfg(windows)]
fn embed_windows_icon() {
    use std::{env, fs, path::PathBuf, process::Command};
    let manifest =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"));
    let icon = manifest
        .join("../../resources/icon/icon.ico")
        .canonicalize()
        .expect("existing Shellfox icon");
    println!("cargo:rerun-if-changed={}", icon.display());
    for name in [
        "SHELLFOX_RC",
        "WindowsSdkVerBinPath",
        "WindowsSdkDir",
        "PATH",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build directory"));
    let source = output.join("shellfox-icon.rc");
    let resource = output.join("shellfox-icon.res");
    let path = icon
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/");
    fs::write(&source, format!("1 ICON \"{path}\"\n")).expect("write Windows icon resource source");
    let compiler=resource_compiler().expect("Windows SDK rc.exe is required; use a Developer PowerShell or set SHELLFOX_RC to its absolute path");
    let status = Command::new(compiler)
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg(&source)
        .status()
        .expect("start Windows resource compiler");
    assert!(status.success(), "compile Shellfox Windows icon resource");
    println!(
        "cargo:rustc-link-arg-bin=shellfox-gpui-experiment={}",
        resource.display()
    );
}
#[cfg(windows)]
fn resource_compiler() -> Option<std::path::PathBuf> {
    use std::{env, path::PathBuf};
    if let Some(explicit) = env::var_os("SHELLFOX_RC") {
        let path = PathBuf::from(explicit);
        return (path.is_absolute() && path.is_file()).then_some(path);
    }
    let arch = if env::var("HOST").ok()?.starts_with("aarch64") {
        "arm64"
    } else {
        "x64"
    };
    if let Some(directory) = env::var_os("WindowsSdkVerBinPath") {
        let path = PathBuf::from(directory).join(arch).join("rc.exe");
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(paths) = env::var_os("PATH") {
        for directory in env::split_paths(&paths) {
            let path = directory.join("rc.exe");
            if path.is_file() {
                return Some(path);
            }
        }
    }
    let sdk = env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("ProgramFiles(x86)")
                .map(|programs| PathBuf::from(programs).join("Windows Kits/10"))
        })?;
    let mut versions: Vec<_> = std::fs::read_dir(sdk.join("bin"))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join(arch).join("rc.exe").is_file())
        .collect();
    versions.sort();
    versions.pop().map(|path| path.join(arch).join("rc.exe"))
}
