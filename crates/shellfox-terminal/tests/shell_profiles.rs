#![cfg(all(windows, feature = "test-support"))]
#[path = "support/pty.rs"]
mod support;
use shellfox_terminal::{
    default_shell_profile, discover_shell_profiles, EnvironmentPolicy, PtySession, ShellProfileId,
    TerminalEnvironment,
};
use std::path::PathBuf;

#[derive(serde::Deserialize)]
struct ShellProbe {
    project: String,
    home: String,
    pi_state: String,
    history: String,
    module_path: String,
    pi_lookup: Option<String>,
    pi_version: Option<String>,
    node_lookup: Option<String>,
}
use support::{drain, Scratch};

#[test]
fn normal_available_shell_defaults_to_powershell_and_explicit_unavailable_fails() {
    let profiles = discover_shell_profiles().unwrap();
    let expected = profiles
        .iter()
        .find(|p| p.unavailable_reason().is_none())
        .unwrap();
    assert_eq!(default_shell_profile().unwrap().id(), expected.id());
    assert!(matches!(
        expected.id(),
        ShellProfileId::Pwsh | ShellProfileId::WindowsPowerShell | ShellProfileId::Cmd
    ));
    let scratch = Scratch::new();
    let mut config = scratch.config("--idle");
    config.command = None;
    config.profile = Some(ShellProfileId::Wsl);
    let error = PtySession::spawn(config).unwrap_err();
    assert!(format!("{error:#}").contains("WSL"));
    assert!(error.take_session().is_none());
}

#[test]
fn interactive_pwsh_keeps_project_and_tool_state_separate() {
    interactive_powershell(ShellProfileId::Pwsh);
}

#[test]
fn interactive_windows_powershell_keeps_project_and_tool_state_separate() {
    interactive_powershell(ShellProfileId::WindowsPowerShell);
}

fn interactive_powershell(id: ShellProfileId) {
    if let Some(profile) = discover_shell_profiles()
        .unwrap()
        .into_iter()
        .find(|p| p.id() == id && p.unavailable_reason().is_none())
    {
        let scratch = Scratch::new();
        let project = Scratch::new();
        std::fs::write(project.0.join("owned-project-marker"), "fixture").unwrap();
        let tools = scratch.0.join("tools");
        std::fs::create_dir(&tools).unwrap();
        std::fs::copy(
            env!("CARGO_BIN_EXE_shellfox-pty-test-helper"),
            tools.join("owned-command.exe"),
        )
        .unwrap();
        std::fs::write(
            tools.join("owned-launcher.ps1"),
            "Write-Output 'OWNED_PS1_LAUNCHER_PASS'\r\n",
        )
        .unwrap();
        let mut config = scratch.config("--idle");
        config.command = None;
        config.profile = Some(profile.id());
        // Host stores often canonicalize paths. Both shell families must still
        // receive ordinary DOS paths rather than verbatim provider paths.
        config.cwd = std::fs::canonicalize(&project.0).unwrap();
        config.environment = TerminalEnvironment::Isolated {
            root: std::fs::canonicalize(&scratch.0).unwrap(),
            policy: EnvironmentPolicy::InstalledTools,
            tool_directories: vec![tools],
        };
        let pi_launcher = std::env::var_os("PNPM_HOME")
            .map(PathBuf::from)
            .map(|p| p.join("pi.cmd"))
            .filter(|p| p.is_absolute() && p.is_file());
        if let Some(path) = &pi_launcher {
            config.env.insert(
                "TEST_PI_VERSION_LAUNCHER".into(),
                path.as_os_str().to_owned(),
            );
        }
        let mut session = PtySession::spawn(config).unwrap();
        // Only the standard metadata flag is authorized, with offline startup
        // and fresh private pi state. Never launch an account-backed agent.
        session.write(concat!(
            "$ErrorActionPreference='Stop'; ",
            "try { ",
            "Write-Output ('LS_ALIAS=' + (Get-Alias ls).Definition); ",
            "Write-Output ('LS_FILE=' + (ls owned-project-marker).Name); ",
            "$probe = @{ project=(Get-Location).ProviderPath; home=$env:HOME; pi_state=$env:PI_CODING_AGENT_DIR; history=(Get-PSReadLineOption).HistorySavePath; module_path=$env:PSModulePath; ",
            "pi_lookup=(Get-Command pi -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Source); ",
            "node_lookup=(Get-Command node -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Source); pi_version=$null }; ",
            "if ($env:TEST_PI_VERSION_LAUNCHER) { ",
            "if ([IO.Path]::GetDirectoryName($probe.pi_lookup) -ne [IO.Path]::GetDirectoryName($env:TEST_PI_VERSION_LAUNCHER)) { throw 'Unexpected pi installation' }; ",
            "$env:PI_OFFLINE='1'; $env:PI_SKIP_VERSION_CHECK='1'; $probe.pi_version=(pi --version | Out-String).Trim(); ",
            "if ($LASTEXITCODE -ne 0) { throw 'pi --version failed' } }; ",
            "[IO.File]::WriteAllText((Join-Path $env:TMP 'shell-probe.json'), ($probe | ConvertTo-Json -Compress)); ",
            "owned-command --short; owned-launcher; ",
            "if (Get-Command node -ErrorAction SilentlyContinue) { $nodeResult=(node -p '20+22' | Out-String).Trim(); if ($LASTEXITCODE -ne 0) { throw 'node arithmetic failed' }; Write-Output ('NODE_RESULT=' + $nodeResult) }; ",
            "Write-Output ('SECRET_PRESENT=' + [bool]$env:CARGO_MANIFEST_DIR); ",
            "Write-Output 'INTERACTIVE_PASS'; exit 0 ",
            "} catch { Write-Output ('INTERACTIVE_FAILURE=' + $_.Exception.Message); exit 21 }\r"
        ).as_bytes()).unwrap();
        let output = String::from_utf8_lossy(&drain(&mut session)).into_owned();
        assert!(
            output.contains("LS_ALIAS=Get-ChildItem"),
            "{}: {output}",
            profile.label()
        );
        assert!(output.contains("LS_FILE=owned-project-marker"), "{output}");
        assert!(output.contains("PTY_FINAL_MARKER"), "{output}");
        assert!(output.contains("INTERACTIVE_PASS"), "{output}");
        assert!(output.contains("OWNED_PS1_LAUNCHER_PASS"), "{output}");
        // ConPTY can wrap long paths with cursor movement. Read the typed owned
        // probe file instead of mistaking that rendering for changed state.
        let probe: ShellProbe = serde_json::from_str(
            &std::fs::read_to_string(scratch.0.join("temp/shell-probe.json")).unwrap(),
        )
        .unwrap();
        if probe.node_lookup.is_some() {
            assert!(output.contains("NODE_RESULT=42"), "{output}");
        }
        if pi_launcher.is_some() {
            assert!(
                probe
                    .pi_lookup
                    .as_deref()
                    .is_some_and(|p| p.contains("pi.")),
                "{output}"
            );
        }
        // Installation and isolated module directories, never the account's.
        let expected_modules = profile
            .executable()
            .unwrap()
            .parent()
            .unwrap()
            .join("Modules");
        let expected_modules = std::fs::canonicalize(expected_modules).unwrap();
        let modules: Vec<_> = std::env::split_paths(&probe.module_path)
            .filter_map(|path| std::fs::canonicalize(path).ok())
            .collect();
        assert!(modules.contains(&expected_modules), "{}", probe.module_path);
        let account = std::path::Path::new(&std::env::var("USERPROFILE").unwrap()).join("Documents");
        let account = account.to_string_lossy().to_ascii_lowercase();
        assert!(
            !probe.module_path.to_ascii_lowercase().contains(&account),
            "{}",
            probe.module_path
        );
        if pi_launcher.is_some() {
            assert!(
                probe.pi_version.as_deref().is_some_and(
                    |v| !v.is_empty() && v.chars().next().is_some_and(|c| c.is_ascii_digit())
                ),
                "pi version missing: {output}"
            );
            eprintln!(
                "{}: isolated offline pi --version = {:?}",
                profile.label(),
                probe.pi_version
            );
        }
        eprintln!(
            "{}: ls alias, owned PATH command, local ps1 launcher and available node passed",
            profile.label()
        );
        eprintln!("MODULE_PATH={}", probe.module_path);
        assert!(output.contains("SECRET_PRESENT=False"), "{output}");
        assert_eq!(PathBuf::from(probe.project), project.0);
        assert_eq!(PathBuf::from(probe.home), scratch.0.join("home"));
        assert_eq!(PathBuf::from(probe.pi_state), scratch.0.join("pi-agent"));
        assert_eq!(
            PathBuf::from(probe.history),
            scratch.0.join("state/powershell-history")
        );
        assert_eq!(
            session.try_wait().unwrap().unwrap().exit_code(),
            0,
            "{output}"
        );
        session.shutdown().unwrap();
        assert_eq!(
            std::fs::read_dir(&project.0).unwrap().count(),
            1,
            "shell state leaked into project"
        );
    }
}

#[test]
fn invalid_lookup_directories_fail_before_spawn() {
    let scratch = Scratch::new();
    let mut config = scratch.config("--idle");
    config.environment = TerminalEnvironment::Isolated {
        root: scratch.0.clone(),
        policy: EnvironmentPolicy::InstalledTools,
        tool_directories: vec![PathBuf::from("relative")],
    };
    assert!(PtySession::spawn(config)
        .unwrap_err()
        .take_session()
        .is_none());
}

#[test]
fn preexisting_config_history_and_directory_links_cannot_escape_isolation() {
    for relative in [
        "config/npmrc",
        "config/npm-globalrc",
        "config/gitconfig",
        "config/gitconfig-system",
        "state/shell-history",
        "state/powershell-history",
    ] {
        let scratch = Scratch::new();
        let outside = Scratch::new();
        let target = outside.0.join("do-not-read-or-write");
        std::fs::write(&target, "owned outside fixture").unwrap();
        let link = scratch.0.join(relative);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::windows::fs::symlink_file(&target, &link)
            .expect("link regression needs Windows symlink support");
        let error = PtySession::spawn(scratch.config("--short")).unwrap_err();
        assert!(error.take_session().is_none(), "{relative}");
        assert!(
            format!("{error:#}").contains("symlink or reparse point"),
            "{relative}: {error:#}"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "owned outside fixture"
        );
    }
    let scratch = Scratch::new();
    let outside = Scratch::new();
    std::os::windows::fs::symlink_dir(&outside.0, scratch.0.join("cache"))
        .expect("link regression needs Windows symlink support");
    let error = PtySession::spawn(scratch.config("--short")).unwrap_err();
    assert!(error.take_session().is_none());
    assert!(format!("{error:#}").contains("symlink or reparse point"));
    assert_eq!(std::fs::read_dir(&outside.0).unwrap().count(), 0);

    let scratch = Scratch::new();
    let outside = Scratch::new();
    let target = outside.0.join("owned-hardlink-fixture");
    std::fs::write(&target, "owned hardlink fixture").unwrap();
    std::fs::create_dir(scratch.0.join("config")).unwrap();
    std::fs::hard_link(&target, scratch.0.join("config/npmrc")).unwrap();
    let error = PtySession::spawn(scratch.config("--short")).unwrap_err();
    assert!(error.take_session().is_none());
    assert!(format!("{error:#}").contains("hardlink aliases"));
    assert_eq!(
        std::fs::read_to_string(target).unwrap(),
        "owned hardlink fixture"
    );

    let scratch = Scratch::new();
    std::fs::create_dir_all(scratch.0.join("config/npmrc")).unwrap();
    let error = PtySession::spawn(scratch.config("--short")).unwrap_err();
    assert!(error.take_session().is_none());
    assert!(format!("{error:#}").contains("unexpected file type"));
}

#[test]
fn explicit_cmd_retains_autorun_disabled_and_can_find_owned_executable() {
    let scratch = Scratch::new();
    let tools = scratch.0.join("tools");
    std::fs::create_dir(&tools).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_shellfox-pty-test-helper"),
        tools.join("owned-command.exe"),
    )
    .unwrap();
    let mut config = scratch.config("--idle");
    config.command = None;
    config.profile = Some(ShellProfileId::Cmd);
    config.environment = TerminalEnvironment::Isolated {
        root: scratch.0.clone(),
        policy: EnvironmentPolicy::InstalledTools,
        tool_directories: vec![tools],
    };
    let mut session = PtySession::spawn(config).unwrap();
    session
        .write(b"owned-command --short & echo CMD_PASS & exit 0\r")
        .unwrap();
    let output = String::from_utf8_lossy(&drain(&mut session)).into_owned();
    assert!(
        output.contains("PTY_FINAL_MARKER") && output.contains("CMD_PASS"),
        "{output}"
    );
    assert_eq!(session.try_wait().unwrap().unwrap().exit_code(), 0);
    session.shutdown().unwrap();
}
