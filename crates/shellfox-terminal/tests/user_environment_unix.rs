//! Linux and macOS user terminals inherit the host's session environment and
//! run the account's login shell, with Shellfox's layers on top.
#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    feature = "test-support"
))]
use shellfox_terminal::{PtyConfig, PtyOutput, PtySession, ShellProfileId};
use std::{
    collections::BTreeMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MARKER: &str = "12345678-1234-4234-8234-123456789abc";

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::create_dir_all(&base).unwrap();
        let root = base.join(format!("user-env-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(root.canonicalize().unwrap())
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs `script` in the tab's shell and returns the variables it saw.
fn run(mut config: PtyConfig, root: &Path, script: &str) -> (BTreeMap<String, String>, String) {
    config.supervisor = Some(PathBuf::from(env!(
        "CARGO_BIN_EXE_shellfox-unix-test-supervisor"
    )));
    let output = root.join("env.txt");
    let mut session = PtySession::spawn(config).unwrap();
    if !script.is_empty() {
        session
            .write(
                script
                    .replace("OUT", &output.display().to_string())
                    .as_bytes(),
            )
            .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut screen = Vec::new();
    while !root.join("done").exists() {
        assert!(
            Instant::now() < deadline,
            "shell did not finish: {}",
            String::from_utf8_lossy(&screen)
        );
        while let Ok(Some(PtyOutput::Data(bytes))) = session.try_read() {
            screen.extend(bytes);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    session.shutdown().unwrap();
    let env = std::fs::read_to_string(&output)
        .unwrap()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    let shell = std::fs::read_to_string(root.join("shell.txt")).unwrap_or_default();
    (env, shell)
}

#[test]
fn user_tabs_keep_the_session_environment_and_run_a_login_shell() {
    // Set before any tab starts; this test binary runs nothing else in parallel
    // that reads these names.
    std::env::set_var("DISPLAY", ":shellfox-test");
    std::env::set_var(
        "DBUS_SESSION_BUS_ADDRESS",
        "unix:path=/tmp/shellfox-test-bus",
    );
    std::env::set_var("SHELLFOX_TEST_INTERNAL", "host-only");
    std::env::set_var("TMUX", "/tmp/tmux-test/default,1,0");
    let scratch = Scratch::new("user");
    let cli = scratch.0.join("cli");
    let tools = scratch.0.join("tools");
    std::fs::create_dir(&cli).unwrap();
    std::fs::create_dir(&tools).unwrap();
    let mut config = PtyConfig::user(&scratch.0);
    config.profile = Some(ShellProfileId::LoginShell);
    config.terminal_marker = Some(MARKER.into());
    config.cli_bin = Some(cli.clone());
    config
        .host_paths
        .insert("SHELLFOX_USER_DATA".into(), scratch.0.clone().into());
    config
        .env
        .insert("CONFIGURED_VALUE".into(), "from-settings".into());
    config.env.insert(
        "PATH".into(),
        format!("{}:/usr/bin:/bin", tools.display()).into(),
    );
    let (env, shell) = run(
        config,
        &scratch.0,
        "env > OUT; ps -o args= -p $$ > OUT.shell; mv OUT.shell \"$(dirname OUT)/shell.txt\"; touch \"$(dirname OUT)/done\"; exit\n",
    );
    let home = std::env::var("HOME").unwrap();
    assert_eq!(env.get("HOME"), Some(&home));
    assert_eq!(
        env.get("DISPLAY").map(String::as_str),
        Some(":shellfox-test")
    );
    assert_eq!(
        env.get("DBUS_SESSION_BUS_ADDRESS").map(String::as_str),
        Some("unix:path=/tmp/shellfox-test-bus")
    );
    assert_eq!(env.get("SHELLFOX_TEST_INTERNAL"), None);
    assert_eq!(env.get("SHELLFOX_PTY_ISOLATED"), None);
    assert_eq!(env.get("TMUX"), None);
    assert_eq!(env.get("TERM").map(String::as_str), Some("xterm-256color"));
    assert_eq!(
        env.get("CONFIGURED_VALUE").map(String::as_str),
        Some("from-settings")
    );
    assert_eq!(
        env.get("SHELLFOX_TERMINAL_MARKER").map(String::as_str),
        Some(MARKER)
    );
    assert_eq!(
        env.get("SHELLFOX_USER_DATA"),
        Some(&scratch.0.display().to_string())
    );
    // Login files may add directories, but the CLI stays before the configured PATH.
    let path: Vec<_> = env["PATH"].split(':').collect();
    let at = |dir: &Path| path.iter().position(|p| Path::new(p) == dir);
    assert!(at(&cli).unwrap() < at(&tools).unwrap(), "{}", env["PATH"]);
    assert!(
        shell.split_whitespace().any(|arg| arg == "-l"),
        "not a login shell: {shell}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn isolated_tabs_get_no_session_bus() {
    let scratch = Scratch::new("isolated");
    let mut config = PtyConfig::isolated(&scratch.0);
    config.command = Some(shellfox_terminal::PtyCommand {
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            "env > OUT; touch \"$(dirname OUT)/done\"".into(),
        ],
    });
    if let Some(command) = &mut config.command {
        let output = scratch.0.join("env.txt").display().to_string();
        command.args[1] = command.args[1]
            .to_string_lossy()
            .replace("OUT", &output)
            .into();
    }
    let (env, _) = run(config, &scratch.0, "");
    assert_eq!(env.get("DBUS_SESSION_BUS_ADDRESS"), None);
    assert_eq!(env.get("SHELLFOX_PTY_ISOLATED"), None);
}
