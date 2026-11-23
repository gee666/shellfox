//! A user terminal gets the user's own environment, not the test process's.
//! These shells run the account's real PowerShell profile, as a user tab does.
#![cfg(all(windows, feature = "test-support"))]
#[path = "support/pty.rs"]
mod support;
use shellfox_terminal::{discover_shell_profiles, PtySession, ShellProfileId, TerminalEnvironment};
use support::{drain, Scratch};

#[derive(serde::Deserialize)]
struct Probe {
    profile: String,
    module_path: String,
    path: String,
    configured: Option<String>,
    host_only: Option<String>,
    registry: Vec<Option<String>>,
    psreadline: bool,
    no_profile: bool,
}

/// Plain REG_SZ values of HKCU\Environment, never edited by this test.
fn registry_values() -> Vec<(String, String)> {
    let output = std::process::Command::new("reg")
        .args(["query", r"HKCU\Environment"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim().splitn(3, "    ");
            let (name, kind, value) = (parts.next()?, parts.next()?, parts.next()?);
            (kind == "REG_SZ" && !name.eq_ignore_ascii_case("Path") && !name.starts_with("SHELLFOX_"))
                .then(|| (name.to_owned(), value.to_owned()))
        })
        .take(4)
        .collect()
}

#[test]
fn powershell_tabs_get_the_user_environment_and_their_profile() {
    // Only this test binary's process has this variable; a fresh user
    // environment must not. The test runner's own PSModulePath may come from
    // a pwsh 7 prompt and must not reach Windows PowerShell either.
    std::env::set_var("USER_ENVIRONMENT_TEST_HOST_ONLY", "leaked");
    let registry = registry_values();
    for id in [ShellProfileId::WindowsPowerShell, ShellProfileId::Pwsh] {
        let Some(profile) = discover_shell_profiles()
            .unwrap()
            .into_iter()
            .find(|p| p.id() == id && p.unavailable_reason().is_none())
        else {
            eprintln!("{id:?} not installed; skipped");
            continue;
        };
        let scratch = Scratch::new();
        let tools = scratch.0.join("tools");
        std::fs::create_dir(&tools).unwrap();
        // The fixture supervisor, with the shell profile and a user environment.
        let mut config = scratch.config("--idle");
        config.command = None;
        config.environment = TerminalEnvironment::User;
        config.profile = Some(id);
        config.env.insert("CONFIGURED_VALUE".into(), "from-settings".into());
        // Settings may prepend to PATH, as in 0.9.
        let system = std::env::var("SystemRoot").unwrap();
        config.env.insert(
            "Path".into(),
            format!(r"{};{system}\System32;{system}", tools.display()).into(),
        );
        let names: Vec<String> = registry.iter().map(|(name, _)| format!("'{name}'")).collect();
        let script = format!(
            "$p = @{{ profile=$env:USERPROFILE; module_path=$env:PSModulePath; path=$env:PATH; \
             configured=$env:CONFIGURED_VALUE; host_only=$env:USER_ENVIRONMENT_TEST_HOST_ONLY; \
             registry=@(@({names}) | % {{ [Environment]::GetEnvironmentVariable($_) }}); \
             psreadline=[bool](Get-Module PSReadLine); \
             no_profile=[bool]([Environment]::GetCommandLineArgs() -match '^-NoProfile$') }}; \
             [IO.File]::WriteAllText('{}', ($p | ConvertTo-Json -Compress)); exit 0\r",
            scratch.0.join("probe.json").display(),
            names = names.join(",")
        );
        let mut session = PtySession::spawn(config).unwrap();
        session.write(script.as_bytes()).unwrap();
        let output = String::from_utf8_lossy(&drain(&mut session)).into_owned();
        session.shutdown().unwrap();
        let probe: Probe = serde_json::from_str(
            &std::fs::read_to_string(scratch.0.join("probe.json"))
                .unwrap_or_else(|error| panic!("{id:?} probe: {error}: {output}")),
        )
        .unwrap();
        let label = profile.label();
        assert_eq!(
            probe.profile.to_ascii_lowercase(),
            std::env::var("USERPROFILE").unwrap().to_ascii_lowercase(),
            "{label}"
        );
        assert_eq!(probe.host_only, None, "{label}: the test process env leaked");
        assert_eq!(probe.configured.as_deref(), Some("from-settings"), "{label}");
        // pwsh adds its own directory in front; the configured PATH follows.
        assert!(probe.path.contains(&format!("{};", tools.display())), "{label}: {}", probe.path);
        for ((name, value), seen) in registry.iter().zip(&probe.registry) {
            assert_eq!(seen.as_deref(), Some(value.as_str()), "{label}: {name}");
        }
        assert!(probe.psreadline, "{label}: PSReadLine is not loaded: {output}");
        assert!(!probe.no_profile, "{label}: started with -NoProfile");
        if id == ShellProfileId::WindowsPowerShell {
            let modules = probe.module_path.to_ascii_lowercase();
            assert!(!modules.contains(r"\powershell\7"), "{label}: {}", probe.module_path);
            assert!(modules.contains(r"\windowspowershell\modules"), "{label}: {}", probe.module_path);
        }
        eprintln!("{label}: user environment, profile and PSReadLine passed");
    }
}
