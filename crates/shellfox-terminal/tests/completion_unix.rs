//! `shellfox cmd bu<Tab>` typed into a real bash or zsh tab completes the
//! registered name. HOME and ZDOTDIR are scratch folders; a stand-in
//! `shellfox` on the tab's CLI path answers `cmd --complete` and logs runs.
#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    feature = "test-support"
))]
use shellfox_terminal::{PtyCommand, PtyConfig, PtyOutput, PtySession, ShellProfileId};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const FAKE_CLI: &str = r#"#!/bin/sh
if [ "$1 $2" = "cmd --complete" ]; then
  for name in buildall lint; do case $name in "$3"*) echo "$name" ;; esac; done
  exit 0
fi
echo "ran $*" >> "$SHELLFOX_TEST_LOG"
"#;

struct Tab {
    root: PathBuf,
    home: PathBuf,
    config: PtyConfig,
}

impl Tab {
    fn new(name: &str) -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::create_dir_all(&base).unwrap();
        let root = base.join(format!("completion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let (home, cli) = (root.join("home"), root.join("cli"));
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&cli).unwrap();
        std::fs::write(cli.join("shellfox"), FAKE_CLI).unwrap();
        std::fs::set_permissions(cli.join("shellfox"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let mut config = PtyConfig::user(&root);
        config.supervisor = Some(PathBuf::from(env!(
            "CARGO_BIN_EXE_shellfox-unix-test-supervisor"
        )));
        config.cli_bin = Some(cli);
        for (key, value) in [
            ("HOME", home.as_os_str()),
            ("SHELLFOX_TEST_LOG", root.join("log").as_os_str()),
        ] {
            config.env.insert(key.into(), value.into());
        }
        // A completer the host's own shell defined must not be the one tested.
        config.env.insert("PROMPT_COMMAND".into(), "".into());
        Self { root, home, config }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.root.join("log")).unwrap_or_default()
    }

    /// Waits for the first prompt, then types each line once the previous
    /// one has left `expect` in the log. Tab gets time to complete.
    fn drive(&self, steps: &[(&str, &str)]) -> String {
        let mut session = PtySession::spawn(self.config.clone()).unwrap();
        let mut screen = Vec::new();
        let read = |session: &PtySession, screen: &mut Vec<u8>| {
            let mut any = false;
            while let Ok(Some(PtyOutput::Data(bytes))) = session.try_read() {
                screen.extend(bytes);
                any = true;
            }
            any
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut quiet = Instant::now();
        while screen.is_empty() || quiet.elapsed() < Duration::from_millis(500) {
            assert!(Instant::now() < deadline, "the shell printed no prompt");
            if read(&session, &mut screen) {
                quiet = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        for (input, expect) in steps {
            for (index, part) in input.split('\t').enumerate() {
                if index > 0 {
                    session.write(b"	").unwrap();
                    std::thread::sleep(Duration::from_millis(500));
                }
                session.write(part.as_bytes()).unwrap();
            }
            let deadline = Instant::now() + Duration::from_secs(30);
            while !self.log().contains(expect) {
                read(&session, &mut screen);
                if Instant::now() > deadline {
                    let _ = session.shutdown();
                    panic!(
                        "no {expect:?} after {input:?}; log {:?}; screen:
{}",
                        self.log(),
                        String::from_utf8_lossy(&screen)
                    );
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        session
            .write(
                b"exit
",
            )
            .unwrap();
        let _ = session.shutdown();
        self.log()
    }
}

impl Drop for Tab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Typed lines: a failed command, the completion, then the state the shell kept.
fn steps(probe: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("echo ready >> \"$SHELLFOX_TEST_LOG\"\n", "ready"),
        ("shellfox cmd bu\t\n", "ran cmd buildall"),
        ("shellfox ver\t\n", "ran version"),
        (probe, "probe"),
    ]
}

#[test]
fn bash_tabs_complete_registered_commands_and_keep_the_status() {
    let mut tab = Tab::new("bash");
    tab.config.profile = Some(ShellProfileId::Bash);
    // The hook keeps $? for the PROMPT_COMMAND the tab inherited.
    tab.config.env.insert(
        "PROMPT_COMMAND".into(),
        "echo \"inherited $?\" >> \"$SHELLFOX_TEST_LOG\"".into(),
    );
    let log = tab.drive(&steps(
        "false\necho \"probe $(complete -p shellfox)\" >> \"$SHELLFOX_TEST_LOG\"\n",
    ));
    assert!(log.contains("inherited 1"), "{log}");
    assert!(
        log.contains("probe complete -F _shellfox_complete shellfox"),
        "{log}"
    );
}

#[test]
fn bash_tabs_keep_a_completer_from_the_users_rc() {
    let mut tab = Tab::new("bash-own");
    tab.config.profile = Some(ShellProfileId::Bash);
    let rc = "_mine() { COMPREPLY=(lint); }\ncomplete -F _mine shellfox\n";
    for name in [".bashrc", ".bash_profile"] {
        std::fs::write(tab.home.join(name), rc).unwrap();
    }
    let log = tab.drive(&[
        ("echo ready >> \"$SHELLFOX_TEST_LOG\"\n", "ready"),
        ("shellfox cmd bu\t\n", "ran cmd lint"),
    ]);
    assert!(!log.contains("buildall"), "{log}");
}

fn zsh() -> Option<PathBuf> {
    let found = shellfox_terminal::shell_profile(ShellProfileId::Zsh).ok();
    if found.is_none() {
        assert!(
            std::env::var_os("SHELLFOX_TEST_REQUIRE_ZSH").is_none(),
            "zsh is required on this runner"
        );
        eprintln!("zsh is not installed; skipping");
    }
    found.and_then(|profile| profile.executable().map(Path::to_path_buf))
}

const ZSHRC: &str = "autoload -Uz compinit && compinit -u -d \"$HOME/.zcompdump\"\n";
const ZSH_PROBE: &str = "false\necho \"probe $? ${ZDOTDIR-unset} ${SHELLFOX_ZDOTDIR-unset} $_comps[shellfox]\" >> \"$SHELLFOX_TEST_LOG\"\n";

#[test]
fn zsh_login_tabs_complete_after_the_users_compinit() {
    if zsh().is_none() {
        return;
    }
    let mut tab = Tab::new("zsh");
    tab.config.profile = Some(ShellProfileId::Zsh);
    std::fs::write(tab.home.join(".zshrc"), ZSHRC).unwrap();
    std::fs::write(
        tab.home.join(".zshenv"),
        "echo \"zshenv ${ZDOTDIR-unset}\" >> \"$SHELLFOX_TEST_LOG\"\n",
    )
    .unwrap();
    let log = tab.drive(&steps(ZSH_PROBE));
    // The user's .zshenv ran with ZDOTDIR as the user had it: unset.
    assert!(log.contains("zshenv unset"), "{log}");
    assert!(
        log.contains("probe 1 unset unset _shellfox_complete"),
        "{log}"
    );
}

#[test]
fn zsh_tabs_keep_the_users_zdotdir() {
    if zsh().is_none() {
        return;
    }
    let mut tab = Tab::new("zsh-zdotdir");
    tab.config.profile = Some(ShellProfileId::Zsh);
    let zdotdir = tab.root.join("zdot");
    std::fs::create_dir(&zdotdir).unwrap();
    std::fs::write(zdotdir.join(".zshrc"), ZSHRC).unwrap();
    std::fs::write(
        zdotdir.join(".zprofile"),
        "echo \"zprofile $ZDOTDIR\" >> \"$SHELLFOX_TEST_LOG\"\n",
    )
    .unwrap();
    tab.config
        .env
        .insert("ZDOTDIR".into(), zdotdir.clone().into());
    let expected = zdotdir.display().to_string();
    let log = tab.drive(&steps(ZSH_PROBE));
    assert!(log.contains(&format!("zprofile {expected}")), "{log}");
    assert!(
        log.contains(&format!("probe 1 {expected} unset _shellfox_complete")),
        "{log}"
    );
}

/// A non-login zsh reads .zshenv and .zshrc only; the hook works the same.
/// The tab command is explicit, so the test sets the hook up as a tab does.
#[test]
fn zsh_non_login_shells_run_the_same_hook() {
    let Some(zsh) = zsh() else { return };
    let mut tab = Tab::new("zsh-plain");
    std::fs::write(tab.home.join(".zshrc"), ZSHRC).unwrap();
    let hook = tab.root.join("shellfox-zsh-test");
    std::fs::create_dir(&hook).unwrap();
    std::fs::write(
        hook.join(".zshenv"),
        shellfox_terminal::completion::zsh_env(),
    )
    .unwrap();
    tab.config.env.insert("ZDOTDIR".into(), hook.clone().into());
    tab.config.command = Some(PtyCommand {
        program: zsh.into(),
        args: vec!["-i".into()],
    });
    let log = tab.drive(&steps(ZSH_PROBE));
    assert!(
        log.contains("probe 1 unset unset _shellfox_complete"),
        "{log}"
    );
    assert!(!hook.exists(), "the hook folder was not removed");
}
