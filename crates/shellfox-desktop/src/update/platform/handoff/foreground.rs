//! Foreground installers for `shellfox update` on Linux and macOS. The package
//! was verified already; the in-app helpers do the same after the app exits.
use anyhow::{Result, ensure};
use std::{path::Path, process::Command};

/// sudo in a terminal, else the PolicyKit prompt the in-app updater uses.
#[cfg(target_os = "linux")]
pub(super) fn apt(package: &Path) -> Result<()> {
    use std::io::IsTerminal;
    let sudo = Path::new("/usr/bin/sudo");
    let mut command = Command::new(if std::io::stdin().is_terminal() && sudo.is_file() {
        sudo
    } else {
        Path::new("/usr/bin/pkexec")
    });
    let status = command
        .args(["/usr/bin/apt-get", "install", "-y"])
        .arg(package)
        .status()?;
    ensure!(status.success(), "apt-get could not install the package");
    Ok(())
}

/// Same staged, codesign-verified renames as install-mac.sh, with rollback.
#[cfg(target_os = "macos")]
pub(super) fn replace_bundle(bundle: &Path, stage: &Path) -> Result<()> {
    let new = stage.join("Shellfox.app");
    let old = stage.join("previous.app");
    let verified = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&new)
        .status()?;
    ensure!(
        verified.success(),
        "The staged app failed signature verification"
    );
    std::fs::rename(bundle, &old)?;
    if let Err(error) = std::fs::rename(&new, bundle) {
        std::fs::rename(&old, bundle).map_err(|restore| {
            anyhow::anyhow!(
                "Rollback failed ({restore}); the previous app is at {}",
                old.display()
            )
        })?;
        return Err(error.into());
    }
    Ok(())
}
