use super::*;
#[test]
fn shims_run_the_cli_and_open_desktop_launches_in_the_gui() {
    let shims = windows_shims(
        r"C:\Program Files\Shellfox.exe",
        r"C:\O'Brien%\bin\shellfox-cli.exe",
    );
    assert!(
        shims
            .cmd
            .contains(r#"start "" "C:\Program Files\Shellfox.exe" %*"#)
    );
    assert!(shims.cmd.ends_with(concat!(
        r#""C:\O'Brien%%\bin\shellfox-cli.exe" %*"#,
        "\r\nexit /b %errorlevel%\r\n"
    )));
    assert!(
        shims
            .powershell
            .contains(r"& 'C:\O''Brien%\bin\shellfox-cli.exe' @args")
    );
    assert!(shims.powershell.ends_with("exit $LASTEXITCODE\n"));
    assert!(shims.posix.contains(
        "exe='C:/Program Files/Shellfox.exe'\ncli='C:/O'\\''Brien%/bin/shellfox-cli.exe'\n"
    ));
    assert!(shims.posix.contains(r#"exe=$(wslpath -u "$exe")"#));
    assert!(shims.posix.ends_with("exec \"$cli\" \"$@\"\n"));
}

/// Runs the generated cmd and PowerShell shims against a stand-in CLI.
#[cfg(windows)]
#[test]
fn windows_shims_forward_cli_output_and_exit_status() {
    let root = tempfile::tempdir().unwrap();
    let cli = root.path().join("fake cli%.cmd");
    std::fs::write(&cli, "@echo off\r\necho cli:%*\r\nexit /b 7\r\n").unwrap();
    let shims = windows_shims(r"C:\missing\Shellfox.exe", cli.to_str().unwrap());
    std::fs::write(root.path().join("shellfox.cmd"), &shims.cmd).unwrap();
    std::fs::write(root.path().join("shellfox.ps1"), &shims.powershell).unwrap();
    let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
    // ssh and --version are console programs too, never the GUI binary.
    for args in [&["session", "list"][..], &["ssh", "host"], &["--version"]] {
        let cmd = Command::new(system.join("cmd.exe"))
            .arg("/d")
            .arg("/c")
            .arg(root.path().join("shellfox.cmd"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(cmd.status.code(), Some(7));
        assert_eq!(
            String::from_utf8_lossy(&cmd.stdout).trim(),
            format!("cli:{}", args.join(" "))
        );
    }
    let ps = Command::new(system.join("WindowsPowerShell/v1.0/powershell.exe"))
        .env_remove("PSModulePath")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(root.path().join("shellfox.ps1"))
        .args(["session", "list"])
        .output()
        .unwrap();
    assert_eq!(ps.status.code(), Some(7));
    assert_eq!(
        String::from_utf8_lossy(&ps.stdout).trim(),
        "cli:session list"
    );
}

/// Runs the generated POSIX shim as WSL would, with a stand-in `wslpath`.
#[test]
fn wsl_folders_reach_windows_as_wsl_paths() {
    let shell = if cfg!(windows) {
        let programs =
            std::env::var_os("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".into());
        std::path::PathBuf::from(programs).join(r"Git\bin\bash.exe")
    } else {
        "/bin/sh".into()
    };
    if !shell.is_file() {
        eprintln!("skipped: no POSIX shell");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    let work = root.path().join("work dir");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(work.join("sub")).unwrap();
    let script = |name: &str, body: &str| {
        let path = bin.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    };
    let log = root.path().join("desktop.log");
    let desktop = script(
        "desktop",
        "for a; do printf '%s\\n' \"$a\"; done > \"$DESKTOP_LOG\"\n",
    );
    let cli = script("cli", "for a; do printf '%s\\n' \"$a\"; done\n");
    // -u keeps the path; -w maps /x/y to \\wsl.localhost\Test\x\y.
    script(
        "wslpath",
        r#"if [ "$1" = -u ]; then printf '%s\n' "$2"; else printf '\\\\wsl.localhost\\Test%s\n' "$(printf '%s' "$2" | tr / '\\')"; fi
"#,
    );
    let shims = windows_shims(desktop.to_str().unwrap(), cli.to_str().unwrap());
    let shim = script("shellfox", shims.posix.trim_start_matches("#!/bin/sh\n"));
    let run = |args: &[&str]| {
        let output = Command::new(&shell)
            .arg(&shim)
            .args(args)
            .current_dir(&work)
            .env("WSL_DISTRO_NAME", "Test")
            .env("DESKTOP_LOG", &log)
            .env("PATH", {
                let mut paths = vec![bin.clone()];
                paths.extend(std::env::split_paths(
                    &std::env::var_os("PATH").unwrap_or_default(),
                ));
                std::env::join_paths(paths).unwrap()
            })
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let desktop_args = |args: &[&str]| {
        let _ = std::fs::remove_file(&log);
        run(args);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !log.is_file() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    // What the shell sees as the work directory, in wslpath -w form.
    let here = run(&["session", "create"])[2].clone();
    assert!(
        here.starts_with(r"\\wsl.localhost\Test\") && here.ends_with(r"\work dir"),
        "{here}"
    );
    assert_eq!(desktop_args(&["start"]), ["start".to_owned(), here.clone()]);
    assert_eq!(
        desktop_args(&["start", "sub", "--new-window"]),
        [
            "start".to_owned(),
            format!(r"{here}\sub"),
            "--new-window".into()
        ]
    );
    assert_eq!(desktop_args(&["new", r"C:\work"]), ["new", r"C:\work"]);
    assert_eq!(
        run(&["session", "create", "--title", "a b", "sub/../sub"]),
        [
            "session",
            "create",
            "--title",
            "a b",
            &format!(r"{here}\sub")
        ]
    );
    assert_eq!(run(&["session", "list"]), ["session", "list"]);
    // The tab marker goes back to the Windows CLI through WSLENV.
    let forwarded = Command::new(&shell)
        .arg("-c")
        .arg(format!(
            "{}\nprintf '%s\\n' \"$WSLENV\"",
            WSL_FOLDER.split("  windows_path()").next().unwrap()
        ))
        .env("WSLENV", "KEEP/p:shellfox_terminal_marker/w")
        .env(
            "SHELLFOX_TERMINAL_MARKER",
            "12345678-1234-4234-8234-123456789abc",
        )
        .env("NO_COLOR", "")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&forwarded.stdout).trim(),
        "KEEP/p:NO_COLOR:SHELLFOX_TERMINAL_MARKER:SHELLFOX_CLI_DISTRO:SHELLFOX_CLI_WSL_CWD"
    );
}
