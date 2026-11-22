//! What a headless package must contain, and safe archive extraction.
//!
//! Each package must also pass the 0.9.x Node validator
//! (src/cli/headless-installation.ts), because an anchor installed as 0.9.x
//! re-validates the selected version on every launch. The Node-only names
//! (`bin/node`, `app/...`, `*.node`, conpty, pagent) are transition files; see
//! scripts/package-rust-headless.mjs.
use crate::headless_layout::read_build;
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use std::{fs, path::Path, process::Command};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Runtime {
    version: u8,
    headless: bool,
    #[serde(rename = "runAsNode")]
    run_as_node: bool,
    node: String,
    entry: String,
    resources: String,
}

fn required_files(platform: &str) -> Vec<String> {
    let win = platform == "win32";
    let suffix = if win { ".exe" } else { "" };
    let mut files: Vec<String> = [
        "app/main.cjs",
        "shellfox",
        "shellfox.cmd",
        "shellfox.ps1",
        "service/install.mjs",
        "app/node_modules/better-sqlite3/build/Release/better_sqlite3.node",
        "app/node_modules/node-pty/build/Release/pty.node",
    ]
    .map(String::from)
    .into();
    files.push(format!("bin/node{suffix}"));
    files.push(format!("bin/shellfox-cli{suffix}"));
    files.push(format!("resources/tailscale/shellfox-tailscale{suffix}"));
    match platform {
        "linux" => files.push("service/shellfox-user.service.in".into()),
        "darwin" => {
            files.extend(darwin_executables().map(String::from));
            files.push("service/com.shellfox.host.plist.in".into());
        }
        _ => files.extend(
            [
                "service/run-host.ps1",
                "util/pagent.exe",
                "app/node_modules/node-pty/build/Release/conpty.node",
                "app/node_modules/node-pty/build/Release/conpty/conpty.dll",
                "app/node_modules/node-pty/build/Release/conpty/OpenConsole.exe",
            ]
            .map(String::from),
        ),
    }
    files
}
fn darwin_executables() -> [&'static str; 3] {
    [
        "app/node_modules/node-pty/build/Release/spawn-helper",
        "resources/terminal-native/shellfox-process-snapshot",
        "resources/terminal-native/shellfox-terminal-supervisor",
    ]
}

/// Checks the identity and the complete file set of an extracted package.
pub fn validate(root: &Path, version: &str, platform: &str, arch: &str) -> Result<()> {
    let build = read_build(root)?;
    ensure!(
        build.version == version && build.platform == platform && build.arch == arch,
        "Headless package identity does not match this release or runtime."
    );
    let runtime: Runtime = serde_json::from_slice(&fs::read(root.join("runtime.json"))?)
        .context("Invalid headless runtime.json")?;
    ensure!(
        runtime.version == 1
            && runtime.headless
            && !runtime.run_as_node
            && runtime.node
                == if platform == "win32" {
                    "bin/node.exe"
                } else {
                    "bin/node"
                }
            && runtime.entry == "app/main.cjs"
            && runtime.resources == "resources",
        "Headless runtime does not match this platform."
    );
    for name in required_files(platform) {
        let info = fs::symlink_metadata(root.join(&name))
            .with_context(|| format!("Headless package is incomplete: {name}"))?;
        ensure!(
            info.is_file() && info.len() > 0,
            "Headless package is incomplete: {name}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let executable = [
                "bin/node",
                "bin/shellfox-cli",
                "shellfox",
                "resources/tailscale/shellfox-tailscale",
            ]
            .contains(&name.as_str())
                || darwin_executables().contains(&name.as_str());
            ensure!(
                !executable || info.permissions().mode() & 0o111 != 0,
                "Headless package executable is not runnable: {name}"
            );
        }
    }
    ensure!(
        root.join("resources").is_dir(),
        "Headless package is incomplete: resources"
    );
    Ok(())
}

fn tar() -> std::path::PathBuf {
    if cfg!(windows) {
        let system = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        Path::new(&system).join("System32").join("tar.exe")
    } else {
        "/usr/bin/tar".into()
    }
}
fn run_tar(args: &[&std::ffi::OsStr]) -> Result<String> {
    let output = Command::new(tar())
        .args(args)
        .env_remove("TAR_OPTIONS")
        .env_remove("TAPE")
        .output()
        .context("Could not run the system tar")?;
    ensure!(
        output.status.success(),
        "tar failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8(output.stdout)?)
}

/// Packages contain only regular files and directories under one root.
pub fn validate_listing(names: &str, details: &str, root: &str) -> Result<()> {
    let lines = |value: &str| -> Vec<String> {
        value
            .replace("\r\n", "\n")
            .split('\n')
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect()
    };
    let (entries, metadata) = (lines(names), lines(details));
    ensure!(
        !entries.is_empty() && entries.len() <= 50_000 && metadata.len() == entries.len(),
        "Invalid headless archive listing."
    );
    let mut seen = std::collections::HashSet::new();
    for (entry, detail) in entries.iter().zip(&metadata) {
        let name = entry.strip_suffix('/').unwrap_or(entry);
        let parts: Vec<&str> = name.split('/').collect();
        let reserved = |part: &str| {
            let stem = part.split('.').next().unwrap_or(part).to_ascii_lowercase();
            matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
                || (stem.len() == 4
                    && (stem.starts_with("com") || stem.starts_with("lpt"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        };
        let unsafe_entry = parts[0] != root
            || parts.iter().any(|p| {
                p.is_empty() || *p == "." || *p == ".." || p.ends_with(['.', ' ']) || reserved(p)
            })
            || name
                .chars()
                .any(|c| c == '\\' || c == ':' || c.is_control())
            || !seen.insert(name.to_ascii_lowercase())
            || !(detail.starts_with('-') || detail.starts_with('d'));
        if unsafe_entry {
            bail!("Unsafe entry in headless archive.");
        }
    }
    Ok(())
}

pub fn extract(archive: &Path, destination: &Path, root: &str) -> Result<std::path::PathBuf> {
    let names = run_tar(&["-tf".as_ref(), archive.as_os_str()])?;
    let details = run_tar(&["-tvf".as_ref(), archive.as_os_str()])?;
    validate_listing(&names, &details, root)?;
    run_tar(&[
        "-xf".as_ref(),
        archive.as_os_str(),
        "--no-same-owner".as_ref(),
        "--no-same-permissions".as_ref(),
        "-C".as_ref(),
        destination.as_os_str(),
    ])?;
    let installation = destination.join(root);
    validate_tree(&installation)?;
    Ok(installation)
}

fn validate_tree(directory: &Path) -> Result<()> {
    let info = fs::symlink_metadata(directory)?;
    ensure!(info.is_dir(), "Invalid headless archive directory.");
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let info = fs::symlink_metadata(&path)?;
        #[cfg(unix)]
        let linked = std::os::unix::fs::MetadataExt::nlink(&info) != 1 && info.is_file();
        #[cfg(not(unix))]
        let linked = false;
        ensure!(
            (info.is_dir() || info.is_file()) && !linked,
            "Unsafe extracted headless entry."
        );
        if info.is_dir() {
            validate_tree(&path)?;
        }
    }
    Ok(())
}

pub fn asset_name(version: &str) -> String {
    let platform = crate::headless_layout::platform();
    let extension = if platform == "win32" { "zip" } else { "tar.gz" };
    format!(
        "shellfox-headless-{version}-{platform}-{}.{extension}",
        crate::headless_layout::arch()
    )
}

#[cfg(test)]
#[path = "headless_package_tests.rs"]
pub(crate) mod tests;
