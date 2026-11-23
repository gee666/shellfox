//! Headless package layout, shared with the 0.9.x Node packages so the
//! original launcher and service definitions keep working across updates.
//!
//! An extracted package is the *anchor*. `shellfox update` installs each new
//! version under `<anchor parent>/shellfox-headless-<ver>-<os>-<arch>-<id>/installation/<root>`,
//! writes `headless-update-origin.json` there and points `<anchor>/headless-update.json`
//! at it. Whatever launcher the anchor has follows that pointer.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

pub const POINTER: &str = "headless-update.json";
pub const ORIGIN: &str = "headless-update-origin.json";
const MANIFEST_LIMIT: u64 = 16 * 1024;

pub fn platform() -> &'static str {
    if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}
pub fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}
pub fn root_name() -> String {
    format!("shellfox-headless-{}-{}", platform(), arch())
}
pub fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Build {
    pub product: String,
    pub version: String,
    pub platform: String,
    pub arch: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    version: u8,
    installation: PathBuf,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Origin {
    version: u8,
    anchor: PathBuf,
}

/// The extracted package this executable belongs to, if it is a headless package.
pub fn current_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bin = exe.parent()?;
    if bin.file_name()? != "bin" {
        return None;
    }
    let root = bin.parent()?;
    read_build(root).ok().filter(|b| b.product == "headless")?;
    Some(normalize(root))
}

pub fn read_build(root: &Path) -> Result<Build> {
    let build: Build = serde_json::from_slice(&read_small(&root.join("build.json"))?)
        .context("Invalid headless build.json")?;
    ensure!(build.product == "headless", "Not a headless package");
    Ok(build)
}

pub fn read_small(file: &Path) -> Result<Vec<u8>> {
    let info = fs::symlink_metadata(file)?;
    ensure!(
        info.is_file() && info.len() <= MANIFEST_LIMIT,
        "Unsafe headless package manifest: {}",
        file.display()
    );
    Ok(fs::read(file)?)
}
fn optional<T: serde::de::DeserializeOwned>(file: &Path) -> Result<Option<T>> {
    match read_small(file) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
            format!("Invalid headless update file: {}", file.display())
        })?)),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// Node's `realpath` spelling, without Windows' `\\?\` verbatim prefix.
pub fn normalize(path: &Path) -> PathBuf {
    realpath(path).unwrap_or_else(|_| path.to_owned())
}
/// Fails like Node's `realpath`, so a path accepted here is accepted by 0.9.x too.
fn realpath(path: &Path) -> std::io::Result<PathBuf> {
    let real = fs::canonicalize(path)?;
    #[cfg(windows)]
    if let Some(text) = real.to_str() {
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{rest}")));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(rest));
        }
    }
    Ok(real)
}
// ASCII case folding matches Node's samePath here: both sides are exact realpath output.
fn same(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
    } else {
        a == b
    }
}

/// Same rule as the 0.9.x launcher: versions live beside the anchor, never elsewhere.
pub fn validate_version_path(anchor: &Path, installation: &Path) -> Result<()> {
    let unsafe_path = || {
        anyhow::anyhow!("Unsafe headless update installation path. Keep the stable installation and its version directories together.")
    };
    ensure!(
        installation.is_absolute() && anchor.is_absolute(),
        unsafe_path()
    );
    let (root, stable) = (
        realpath(installation).map_err(|_| unsafe_path())?,
        realpath(anchor).map_err(|_| unsafe_path())?,
    );
    ensure!(
        same(&root, installation) && same(&stable, anchor),
        unsafe_path()
    );
    let parent = root.parent().ok_or_else(unsafe_path)?;
    let version = parent.parent().ok_or_else(unsafe_path)?;
    ensure!(
        parent.file_name().is_some_and(|n| n == "installation")
            && version
                .parent()
                .zip(stable.parent())
                .is_some_and(|(a, b)| same(a, b))
            && version
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(version_directory_name),
        unsafe_path()
    );
    Ok(())
}
fn version_directory_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("shellfox-headless-") else {
        return false;
    };
    let parts: Vec<_> = rest.splitn(4, '-').collect();
    let [version, os, arch, id] = parts.as_slice() else {
        return false;
    };
    let numbers: Vec<_> = version.split('.').collect();
    numbers.len() == 3
        && numbers
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        && matches!(*os, "win32" | "darwin" | "linux")
        && matches!(*arch, "x64" | "arm64")
        && !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The original installation that owns the stable pointer.
pub fn anchor(root: &Path) -> Result<PathBuf> {
    let Some(origin) = optional::<Origin>(&root.join(ORIGIN))? else {
        return Ok(root.to_owned());
    };
    ensure!(origin.version == 1, "Unsupported headless update origin");
    validate_version_path(&origin.anchor, root)?;
    let (stable, current) = (read_build(&origin.anchor)?, read_build(root)?);
    ensure!(
        stable.platform == current.platform && stable.arch == current.arch,
        "Headless update origin does not match this runtime."
    );
    Ok(origin.anchor)
}

/// The selected version, when it differs from `root`.
pub fn active(root: &Path) -> Result<Option<PathBuf>> {
    let anchor = anchor(root)?;
    let Some(pointer) = optional::<Pointer>(&anchor.join(POINTER))? else {
        return Ok(None);
    };
    ensure!(pointer.version == 1, "Unsupported headless update pointer");
    validate_version_path(&anchor, &pointer.installation)?;
    ensure!(
        same(&self::anchor(&pointer.installation)?, &anchor),
        "Headless update pointer does not belong to this installation."
    );
    Ok((!same(&pointer.installation, root)).then_some(pointer.installation))
}

/// Mark `installation` as a version of `anchor`, then atomically select it.
pub fn activate(anchor: &Path, installation: &Path) -> Result<()> {
    validate_version_path(anchor, installation)?;
    let origin = serde_json::to_vec(&Origin {
        version: 1,
        anchor: anchor.to_owned(),
    })?;
    write_private(&installation.join(ORIGIN), &origin, true)?;
    let mut pointer = serde_json::to_vec(&Pointer {
        version: 1,
        installation: installation.to_owned(),
    })?;
    pointer.push(b'\n');
    let temporary = anchor.join(format!("headless-update-{}.json", uuid::Uuid::new_v4()));
    let result = write_private(&temporary, &pointer, true)
        .and_then(|()| Ok(fs::rename(&temporary, anchor.join(POINTER))?));
    let _ = fs::remove_file(&temporary);
    #[cfg(unix)]
    if result.is_ok() {
        fs::File::open(anchor)?.sync_all()?;
    }
    result
}

pub fn write_private(file: &Path, bytes: &[u8], sync: bool) -> Result<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut handle = options
        .open(file)
        .with_context(|| format!("Could not create {}", file.display()))?;
    handle.write_all(bytes)?;
    if sync {
        handle.sync_all()?;
    }
    Ok(())
}

/// The 0.9.x launcher runs `bin/node app/main.cjs ARGS`; `bin/node` is this
/// CLI in Rust packages. Drop the entry argument it adds. Transition-only.
pub fn strip_node_entry(args: &mut Vec<OsString>) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if exe.file_stem().is_none_or(|stem| stem != "node") {
        return;
    }
    let Some(root) = exe.parent().and_then(Path::parent) else {
        return;
    };
    let entry = normalize(&root.join("app").join("main.cjs"));
    if args
        .first()
        .is_some_and(|first| same(&normalize(Path::new(first)), &entry))
    {
        args.remove(0);
    }
}

/// Forward to the selected version, like the 0.9.x launcher did. Unix replaces
/// this process; Windows waits and the child watches this process.
pub fn forward(args: &[OsString]) -> Result<Option<i32>> {
    let Some(root) = current_root() else {
        return Ok(None);
    };
    let Some(target) = active(&root)? else {
        return Ok(None);
    };
    let build = read_build(&target)?;
    if build.platform != platform() || build.arch != arch() {
        bail!("Activated headless runtime does not match this machine.");
    }
    let cli = target.join("bin").join(exe("shellfox-cli"));
    let mut command = std::process::Command::new(&cli);
    command.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec()).with_context(|| format!("Unable to launch {}", cli.display()))
    }
    #[cfg(not(unix))]
    {
        command.env(
            crate::parent_watch::FORWARDER,
            std::process::id().to_string(),
        );
        // Console Ctrl+C reaches both processes; the child decides how to stop.
        crate::parent_watch::ignore_console_interrupts();
        let status = command
            .status()
            .with_context(|| format!("Unable to launch {}", cli.display()))?;
        Ok(Some(status.code().unwrap_or(1)))
    }
}

#[cfg(test)]
#[path = "headless_layout_tests.rs"]
mod tests;
