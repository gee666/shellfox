use std::{
    fs,
    os::windows::process::CommandExt,
    process::Command,
    time::{Duration, Instant},
};

/// Runs the real Windows helper: it must wait for this version's terminal host
/// to exit before it starts the installer.
#[test]
fn windows_helper_installs_only_after_the_desktop_host_exits() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("app-1.0.0/bin");
    fs::create_dir_all(&bin).unwrap();
    let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
    let cli = bin.join("shellfox-cli.exe");
    fs::copy(system.join("cmd.exe"), &cli).unwrap();
    // Stand-in desktop host under the install root, alive ~2 s.
    let mut host = Command::new(&cli)
        .raw_arg("/d /c ping -n 3 127.0.0.1 >nul")
        .spawn()
        .unwrap();
    let mut ui = Command::new(system.join("cmd.exe"))
        .args(["/d", "/c", "exit"])
        .spawn()
        .unwrap();
    ui.wait().unwrap();
    let setup = root.path().join("setup.cmd");
    fs::write(
        &setup,
        "@echo off\r\necho done> \"%~dp0installed\"\r\nexit /b 0\r\n",
    )
    .unwrap();
    use sha2::Digest;
    let sha256 = format!("{:x}", sha2::Sha256::digest(fs::read(&setup).unwrap()));
    let (work, control) = (root.path().join("work"), root.path().join("control"));
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(&control).unwrap();
    fs::write(control.join("commit"), "").unwrap();
    let helper = root.path().join("install.ps1");
    fs::write(&helper, include_str!("install-windows.ps1")).unwrap();
    let config = control.join("install.json");
    fs::write(&config, serde_json::to_vec(&serde_json::json!({
        "pid": ui.id(), "file": setup, "work": work, "updateExe": root.path().join("Update.exe"),
        "sha256": sha256, "control": control, "relaunch": false,
    })).unwrap()).unwrap();
    let started = Instant::now();
    let status = super::windows_helper(&helper, &config)
        .unwrap()
        .output()
        .unwrap();
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
    let status = status.status;
    let host_exited = host.try_wait().unwrap().is_some();
    let _ = host.kill();
    assert!(status.success(), "helper failed: {output}");
    assert!(host_exited, "installer started while the host was running");
    assert!(started.elapsed() >= Duration::from_millis(1500));
    assert!(root.path().join("installed").is_file());
    assert!(!work.exists(), "helper keeps its work directory");
}
