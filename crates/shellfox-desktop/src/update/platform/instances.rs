//! Processes of one installation: its UI, its desktop host and CLI calls.
//! `shellfox update` refuses to install over them; this process and the
//! shellfox-cli that forwarded the command don't count.
use anyhow::Result;
#[cfg(any(windows, target_os = "macos"))]
use anyhow::ensure;
use std::path::Path;

/// Pids other than ours and our parent's.
fn others(processes: impl IntoIterator<Item = u32>, parent: Option<u32>) -> bool {
    let me = std::process::id();
    processes
        .into_iter()
        .any(|pid| pid != me && Some(pid) != parent)
}

/// Windows: any process running an executable under the Squirrel root, which
/// Setup.exe would otherwise kill.
#[cfg(windows)]
pub(super) fn running(root: &Path) -> Result<bool> {
    use std::os::windows::process::CommandExt;
    let system = std::env::var_os("SystemRoot").map(std::path::PathBuf::from);
    let powershell = system
        .ok_or_else(|| anyhow::anyhow!("Windows system root is unavailable"))?
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let script = "$root = $env:SHELLFOX_UPDATE_ROOT.TrimEnd('\\') + '\\'
Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root, [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { \"$($_.ProcessId) $($_.ParentProcessId)\" }";
    let output = std::process::Command::new(powershell)
        .creation_flags(0x0800_0000)
        .env_remove("PSModulePath")
        .env("SHELLFOX_UPDATE_ROOT", root)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .output()?;
    ensure!(output.status.success(), "Could not list Shellfox processes");
    let pairs: Vec<(u32, u32)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, parent) = line.trim().split_once(' ')?;
            Some((pid.parse().ok()?, parent.parse().ok()?))
        })
        .collect();
    let me = std::process::id();
    let parent = pairs
        .iter()
        .find(|(pid, _)| *pid == me)
        .map(|(_, parent)| *parent);
    Ok(others(pairs.into_iter().map(|(pid, _)| pid), parent))
}

/// Linux: the desktop binary or shellfox-cli of this package.
#[cfg(target_os = "linux")]
pub(super) fn running(directory: &Path) -> Result<bool> {
    let pids = std::fs::read_dir("/proc")?.filter_map(|entry| {
        let entry = entry.ok()?;
        let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
        let exe = std::fs::read_link(entry.path().join("exe")).ok()?;
        let name = exe.file_name()?.to_str()?;
        (exe.parent() == Some(directory) && matches!(name, "shellfox" | "shellfox-cli"))
            .then_some(pid)
    });
    Ok(others(
        pids.collect::<Vec<_>>(),
        Some(std::os::unix::process::parent_id()),
    ))
}

/// macOS: anything started from the bundle's executables.
#[cfg(target_os = "macos")]
pub(super) fn running(bundle: &Path) -> Result<bool> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-x", "-o", "pid=,args="])
        .output()?;
    ensure!(output.status.success(), "Could not list Shellfox processes");
    let prefix = format!("{}/Contents/MacOS/", bundle.display());
    let pids: Vec<u32> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, args) = line.trim_start().split_once(' ')?;
            args.trim_start()
                .starts_with(&prefix)
                .then(|| pid.parse().ok())
                .flatten()
        })
        .collect();
    Ok(others(pids, Some(std::os::unix::process::parent_id())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_other_processes_count() {
        let me = std::process::id();
        assert!(!others([me, 7], Some(7)));
        assert!(others([me, 8], Some(7)));
        assert!(!others([], None));
    }

    #[cfg(windows)]
    #[test]
    fn finds_a_process_running_from_the_install_root() {
        let root = tempfile::tempdir().unwrap();
        let probe = root.path().join("app-1.0.0/bin/probe.exe");
        std::fs::create_dir_all(probe.parent().unwrap()).unwrap();
        let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        std::fs::copy(system.join("System32/cmd.exe"), &probe).unwrap();
        assert!(!running(root.path()).unwrap());
        let mut child = std::process::Command::new(&probe)
            .args(["/d", "/c", "ping -n 30 127.0.0.1 >nul"])
            .spawn()
            .unwrap();
        let found = running(root.path());
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(found.unwrap());
    }
}
