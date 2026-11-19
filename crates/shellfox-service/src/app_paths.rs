//! Per-user data and producer-pinned resources shared by the installed desktop and CLI.
use anyhow::{ensure, Context, Result};
use std::{env, path::PathBuf};

/// The same app-data base Electron uses, without opening Electron's data files.
pub fn default_data_root() -> Result<PathBuf> {
    let base = if cfg!(windows) {
        absolute_env("APPDATA")?
    } else if cfg!(target_os = "macos") {
        absolute_env("HOME")?.join("Library/Application Support")
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(Ok)
            .unwrap_or_else(|| absolute_env("HOME").map(|home| home.join(".config")))?
    };
    Ok(base.join("Shellfox/rust-v1/host"))
}

fn absolute_env(name: &str) -> Result<PathBuf> {
    let path = PathBuf::from(env::var_os(name).with_context(|| format!("{name} is not set"))?);
    ensure!(path.is_absolute(), "{name} must be absolute");
    Ok(path)
}

pub const fn is_packaged() -> bool {
    option_env!("SHELLFOX_PACKAGED_GO_SHA256").is_some()
}

/// Locate the baked-in helper, never accepting a digest read from the install directory.
pub fn packaged_helper() -> Result<(PathBuf, [u8; 32])> {
    let expected = option_env!("SHELLFOX_PACKAGED_GO_SHA256")
        .context("Build has no packaged helper; supply --helper and --helper-sha256")?;
    let digest = parse_sha256(expected)?;
    let exe = env::current_exe()?;
    let dir = exe
        .parent()
        .context("Missing executable parent directory")?;
    let helper = if cfg!(windows) {
        "shellfox-tailscale.exe"
    } else {
        "shellfox-tailscale"
    };
    let resources = if cfg!(target_os = "macos") {
        dir.join("../Resources")
    } else if cfg!(windows) && dir.ends_with("bin") {
        dir.join("../resources")
    } else {
        dir.join("resources")
    };
    let candidate = resources.canonicalize()?.join("tailscale").join(helper);
    #[cfg(windows)]
    let candidate = stable_windows_helper(candidate, digest)?;
    Ok((candidate, digest))
}

fn parse_sha256(value: &str) -> Result<[u8; 32]> {
    ensure!(
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid packaged helper digest"
    );
    let mut bytes = [0; 32];
    for (byte, pair) in bytes.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair)?, 16)?;
    }
    Ok(bytes)
}

#[cfg(windows)]
fn stable_windows_helper(candidate: PathBuf, digest: [u8; 32]) -> Result<PathBuf> {
    use shellfox_transport::VerifiedHelper;
    use std::{fs, io::Write, os::windows::fs::MetadataExt};

    // Match Electron's cache so the firewall executable path does not change.
    let arch = if cfg!(target_arch = "aarch64") {
        "win32-arm64"
    } else {
        "win32-x64"
    };
    let directory = absolute_env("LOCALAPPDATA")?
        .join("Shellfox/helpers")
        .join(arch);
    for dir in directory
        .ancestors()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        match fs::create_dir(dir) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        let info = fs::symlink_metadata(dir)?;
        ensure!(
            info.is_dir() && info.file_attributes() & 0x400 == 0,
            "Invalid Shellfox helper directory: {}",
            dir.display()
        );
    }
    let target = directory.join("shellfox-tailscale.exe");
    if VerifiedHelper::new(target.clone(), digest).is_ok() {
        return Ok(target);
    }
    let source = VerifiedHelper::new(candidate, digest)
        .context("Packaged Shellfox helper checksum mismatch")?;
    let bytes = source.read_verified(64 * 1024 * 1024)?;
    let mut staged = tempfile::NamedTempFile::new_in(&directory)?;
    staged.write_all(&bytes)?;
    staged.as_file().sync_all()?;
    match staged.persist(&target) {
        Err(error) if VerifiedHelper::new(target.clone(), digest).is_err() => {
            return Err(error).context("Could not replace the stable Shellfox helper. Close other Shellfox hosts and try again");
        }
        _ => (),
    }
    VerifiedHelper::new(target.clone(), digest)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_separates_rust_from_electron() {
        assert!(default_data_root()
            .unwrap()
            .ends_with("Shellfox/rust-v1/host"));
    }

    #[test]
    fn packaged_hash_must_be_an_exact_digest() {
        assert_eq!(parse_sha256(&"ab".repeat(32)).unwrap(), [0xab; 32]);
        assert!(parse_sha256(&"$a".repeat(32)).is_err());
        assert!(parse_sha256("ab").is_err());
    }
}
