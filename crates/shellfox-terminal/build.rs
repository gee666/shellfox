//! Stages Microsoft's ConPTY (conpty.dll + OpenConsole.exe) beside Windows
//! build outputs, so dev builds and tests use the same ConPTY as the packages.
//! portable-pty loads it from the executable's directory.
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const VERSION: &str = "1.25.261109001";
const SHA256: &str = "3d77f535df848e44225c2a8912ac844fecb23f3057732b639f3034b587496f09";

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }
    let arch = match env::var("CARGO_CFG_TARGET_ARCH")?.as_str() {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => return Err(format!("no bundled ConPTY for {other}").into()),
    };
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("../..");
    let package = root.join(format!(
        "vendor/conpty/microsoft.windows.console.conpty.{VERSION}.nupkg"
    ));
    println!("cargo:rerun-if-changed={}", package.display());
    let actual: String = Sha256::digest(fs::read(&package)?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if actual != SHA256 {
        return Err(format!("SHA-256 mismatch for {}", package.display()).into());
    }
    let out = PathBuf::from(env::var("OUT_DIR")?);
    let extraction = out.join("conpty");
    if extraction.exists() {
        fs::remove_dir_all(&extraction)?;
    }
    fs::create_dir_all(&extraction)?;
    let dll = format!("runtimes/win-{arch}/native/conpty.dll");
    let host = format!("build/native/runtimes/{arch}/OpenConsole.exe");
    let tar = match env::var_os("SystemRoot") {
        Some(system) if cfg!(windows) => PathBuf::from(system).join("System32/tar.exe"),
        _ => PathBuf::from("bsdtar"),
    };
    let status = Command::new(tar)
        .arg("-xf")
        .arg(&package)
        .arg("-C")
        .arg(&extraction)
        .args([&dll, &host])
        .status()?;
    if !status.success() {
        return Err(format!("cannot extract {}", package.display()).into());
    }
    // OUT_DIR is <profile>/build/<package>/out; tests run from <profile>/deps.
    let profile = out.ancestors().nth(3).ok_or("unexpected OUT_DIR layout")?;
    for destination in [profile.to_path_buf(), profile.join("deps")] {
        fs::create_dir_all(&destination)?;
        stage(&extraction.join(&dll), &destination.join("conpty.dll"))?;
        stage(
            &extraction.join(&host),
            &destination.join("OpenConsole.exe"),
        )?;
    }
    Ok(())
}

/// Leaves an identical file alone: a running dev build keeps it open.
fn stage(source: &Path, destination: &Path) -> Result<()> {
    if destination.is_file() && fs::read(source)? == fs::read(destination)? {
        return Ok(());
    }
    fs::copy(source, destination).map_err(|e| {
        format!(
            "cannot stage {}: {e}. Close running Shellfox builds first",
            destination.display()
        )
    })?;
    Ok(())
}
