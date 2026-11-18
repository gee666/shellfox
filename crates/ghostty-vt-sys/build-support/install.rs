use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const WINDOWS_ARCHIVE: &str = "ghostty-vt-1.0.1-x86_64-windows-gnu.zip";
const WINDOWS_SHA256: &str = "8e2eb9e1822e85d786b456c8beb86f49d09c318c91f0c2913f91e7ffa12cda29";

pub fn windows_archive(root: &Path, out: &Path) -> Result<PathBuf> {
    let archive = root.join("build-support/vendor").join(WINDOWS_ARCHIVE);
    println!("cargo:rerun-if-changed={}", archive.display());
    let mut file = fs::File::open(&archive).map_err(|e| format!(
        "cannot open {}: {e}. Run node crates/ghostty-vt-sys/build-support/prepare.mjs fetch, or set GHOSTTY_VT_DIR to a prepared 1.0.1 installation", archive.display()
    ))?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let actual: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if actual != WINDOWS_SHA256 {
        return Err(format!(
            "SHA-256 mismatch for {}. Refusing to extract it",
            archive.display()
        )
        .into());
    }
    // Always re-extract after Cargo reruns this script. Never trust a stale extraction stamp.
    let extraction = out.join("official-vt-1.0.1");
    if extraction.exists() {
        fs::remove_dir_all(&extraction)?;
    }
    fs::create_dir_all(&extraction)?;
    let mut command = if cfg!(windows) {
        let system = env::var_os("SystemRoot")
            .ok_or("SystemRoot is missing; Windows tar.exe is required")?;
        Command::new(PathBuf::from(system).join("System32/tar.exe"))
    } else {
        // A cross-build host needs bsdtar: GNU tar does not read ZIP archives.
        Command::new("bsdtar")
    };
    let mut child = command
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&extraction)
        .spawn()?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= Duration::from_secs(120) {
            child.kill()?;
            child.wait()?;
            return Err("official Windows ZIP extraction exceeded 120 seconds".into());
        }
        thread::sleep(Duration::from_millis(50));
    };
    if !status.success() {
        return Err("official Windows ZIP extraction failed; install Windows tar.exe or bsdtar on cross-build hosts".into());
    }
    Ok(extraction.join("ghostty-vt-1.0.1-x86_64-windows-gnu"))
}

pub fn validate(prefix: &Path, lib: &Path, target: &str, os: &str, kind: &str) -> Result<()> {
    required(&prefix.join("include/ghostty/vt.h"))?;
    let version = prefix.join("include/ghostty/vt/version.h");
    track(&version);
    let header = fs::read_to_string(&version)?;
    for (name, expected) in [
        ("GHOSTTY_VT_VERSION", "\"1.0.1\""),
        ("GHOSTTY_VT_VERSION_MAJOR", "1"),
        ("GHOSTTY_VT_VERSION_MINOR", "0"),
        ("GHOSTTY_VT_VERSION_PATCH", "1"),
    ] {
        let found = header.lines().find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next() == Some("#define") && fields.next() == Some(name))
                .then(|| fields.next())
                .flatten()
        });
        if found != Some(expected) {
            return Err(format!("{} must declare {name} as {expected}", version.display()).into());
        }
    }
    let marker = prefix.join("ghostty-vt-target.txt");
    track(&marker);
    if marker.exists() && fs::read_to_string(&marker)?.trim() != target {
        return Err(format!(
            "{} was prepared for another Rust target, expected {target}",
            prefix.display()
        )
        .into());
    }
    let pc = lib.join(if kind == "static" {
        "pkgconfig/libghostty-vt-static.pc"
    } else {
        "pkgconfig/libghostty-vt.pc"
    });
    track(&pc);
    let metadata = fs::read_to_string(&pc).map_err(|e| {
        format!(
            "{} is required to validate the installation version: {e}",
            pc.display()
        )
    })?;
    if pc_field(&metadata, "Version") != Some("1.0.1") {
        return Err(format!("{} must have Version: 1.0.1", pc.display()).into());
    }
    // Default upstream builds bundle dependencies. Don't silently omit external static dependencies.
    if kind == "static"
        && ["Libs.private", "Requires.private", "Requires"]
            .iter()
            .any(|key| pc_field(&metadata, key).is_some_and(|value| !value.is_empty()))
    {
        return Err("prepared static library uses external dependencies; rebuild with bundled dependencies using prepare.mjs".into());
    }
    let filename = match (os, kind) {
        ("windows", _) => "ghostty-vt.lib",
        (_, "static") => "libghostty-vt.a",
        ("macos", _) => "libghostty-vt.dylib",
        (_, _) => "libghostty-vt.so",
    };
    required(&lib.join(filename))?;
    if os == "windows" {
        required(&prefix.join("bin/ghostty-vt.dll"))?;
    }
    track(&prefix.join("include"));
    Ok(())
}

fn pc_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == key).then(|| value.trim())
    })
}

fn required(path: &Path) -> Result<()> {
    track(path);
    if !path.is_file() {
        return Err(format!(
            "required libghostty-vt input is missing: {}",
            path.display()
        )
        .into());
    }
    Ok(())
}

fn track(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
}

pub fn stage_windows_runtime(prefix: &Path, out: &Path, notices: &Path) -> Result<()> {
    // OUT_DIR is <target>/<optional triple>/<profile>/build/<package>/out.
    // Do not use PROFILE: custom Cargo profile names and target-dir overrides are valid.
    let profile = out
        .ancestors()
        .nth(3)
        .ok_or("unexpected Cargo OUT_DIR layout")?;
    if out.file_name().is_none_or(|name| name != "out")
        || out
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .is_none_or(|name| name != "build")
    {
        return Err(format!(
            "cannot locate Cargo profile directory from {}",
            out.display()
        )
        .into());
    }
    let source = prefix.join("bin/ghostty-vt.dll");
    for destination in [profile.to_path_buf(), profile.join("deps")] {
        fs::create_dir_all(&destination)?;
        copy_if_different(&source, &destination.join("ghostty-vt.dll"))?;
        copy_tree(notices, &destination.join("ghostty-vt-notices"))?;
    }
    Ok(())
}

fn copy_if_different(source: &Path, destination: &Path) -> Result<()> {
    if destination.is_file() && fs::read(source)? == fs::read(destination)? {
        return Ok(());
    }
    fs::copy(source, destination).map_err(|e| {
        format!(
            "cannot stage {} at {}: {e}. Close running consumers before replacing the DLL",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        } else {
            copy_if_different(&entry.path(), &destination.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "compiler-path.rs"]
mod compiler_path;
#[cfg(test)]
#[path = "experimental-install.rs"]
mod experimental_install;
#[cfg(test)]
#[path = "install-tests.rs"]
mod tests;
