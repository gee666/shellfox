#[path = "../product_version.rs"]
mod product_version;

fn main() {
    let _version = product_version::export();
    #[cfg(windows)]
    embed_windows_icon(&_version);
}

#[cfg(windows)]
fn embed_windows_icon(version: &str) {
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
    let numbers = version.replace('.', ",");
    fs::write(
        &source,
        format!(
            r#"1 ICON "{path}"
1 VERSIONINFO
FILEVERSION {numbers},0
PRODUCTVERSION {numbers},0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "Shellfox\0"
      VALUE "ProductName", "Shellfox\0"
      VALUE "FileVersion", "{version}\0"
      VALUE "ProductVersion", "{version}\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#
        ),
    )
    .expect("write Windows Shellfox resource source");
    let compiler=resource_compiler().expect("Windows SDK rc.exe is required; use a Developer PowerShell or set SHELLFOX_RC to its absolute path");
    let status = Command::new(compiler)
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg(&source)
        .status()
        .expect("start Windows resource compiler");
    assert!(status.success(), "compile Shellfox Windows icon resource");
    println!("cargo:rustc-link-arg-bin=shellfox={}", resource.display());
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
