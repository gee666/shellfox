//! `shellfox` Tab completion: the scripts `shellfox completion <shell>` prints
//! (0.9.15's, byte for byte), and the startup hooks that load them in every
//! Shellfox tab without touching the user's startup files.
//! The scripts only list names; `shellfox cmd --complete` supplies the
//! registered command names when Tab is pressed.
use std::ffi::{OsStr, OsString};

pub const POWERSHELL: &str = include_str!("completion/powershell.txt");
pub const BASH: &str = include_str!("completion/bash.txt");
pub const ZSH: &str = include_str!("completion/zsh.txt");

pub fn script(shell: &str) -> Option<&'static str> {
    match shell {
        "powershell" => Some(POWERSHELL),
        "bash" => Some(BASH),
        "zsh" => Some(ZSH),
        _ => None,
    }
}

/// A PowerShell tab's -Command script runs after the user's $PROFILE; the
/// launch appends the completer to it. Profiles list only the short prompt
/// hook, as their arguments have a size limit. Registering again is harmless.
#[cfg(windows)]
pub(crate) fn add_powershell_completion(command: &mut crate::PtyCommand) {
    let script = command
        .args
        .iter()
        .position(|arg| arg == "-Command")
        .and_then(|at| command.args.get_mut(at + 1));
    if let Some(script) = script {
        script.push(POWERSHELL);
    }
}

/// A bash tab's PROMPT_COMMAND. At the first prompt, after the rc files, it
/// loads completion unless `shellfox` already has a completer. `cwd_hook`
/// adds the OSC 7 hook. The previous `$?` is kept for the commands after it,
/// including an inherited PROMPT_COMMAND. An rc file that assigns
/// PROMPT_COMMAND without keeping the old value still replaces it.
pub fn bash_prompt_command(cwd_hook: bool, inherited: Option<&OsStr>) -> OsString {
    // interactive_comments may be off, so no comment lines.
    let script: String = BASH
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .flat_map(|line| [line, "\n"])
        .collect();
    let mut value = include_str!("completion/bash-load.sh").replace("@COMPLETION@\n", &script);
    value.push_str(if cwd_hook {
        include_str!("bash-prompt.sh")
    } else {
        "(exit \"$__shellfox_status\")\n"
    });
    let mut value = OsString::from(value.trim_end());
    if let Some(inherited) = inherited.filter(|value| !value.is_empty()) {
        value.push("\n");
        value.push(inherited);
    }
    value
}

/// The `.zshenv` of a zsh tab's temporary ZDOTDIR, named `shellfox-zsh-*`.
/// It restores the user's ZDOTDIR from SHELLFOX_ZDOTDIR (or unsets it) before
/// any user file runs, removes its folder, then sources the user's .zshenv.
/// Completion loads at the first prompt with compinit, unless a completer exists.
pub fn zsh_env() -> String {
    include_str!("completion/zshenv.zsh").replace("@COMPLETION@\n", ZSH)
}

/// Name of the variable that carries the user's own ZDOTDIR past the hook.
pub const ZSH_SAVED_ZDOTDIR: &str = "SHELLFOX_ZDOTDIR";
/// Prefix of the temporary ZDOTDIR folder; the hook only removes such a folder.
pub const ZSH_HOOK_PREFIX: &str = "shellfox-zsh-";

/// Adds the startup hooks to a bash or zsh tab started from a shell profile:
/// cwd reporting and completion for bash, completion for a user zsh (an
/// isolated one runs with -f, which reads no ZDOTDIR). A zsh hook that
/// cannot be written only costs completion.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn add_unix_hooks(
    builder: &mut portable_pty::CommandBuilder,
    program: &std::path::Path,
    user: bool,
) {
    match program.file_name().and_then(OsStr::to_str) {
        Some("bash") => {
            let value = bash_prompt_command(true, builder.get_env("PROMPT_COMMAND"));
            builder.env("PROMPT_COMMAND", value);
        }
        Some("zsh") if user => match zsh_hook_dir() {
            Ok(dir) => {
                if let Some(own) = builder.get_env("ZDOTDIR").map(OsStr::to_owned) {
                    builder.env(ZSH_SAVED_ZDOTDIR, own);
                } else {
                    builder.env_remove(ZSH_SAVED_ZDOTDIR);
                }
                builder.env("ZDOTDIR", dir);
            }
            Err(error) => eprintln!("zsh tab starts without shellfox completion: {error:#}"),
        },
        _ => {}
    }
}

/// A private folder in the temporary directory holding the hook's `.zshenv`.
/// The hook removes it as soon as zsh reads it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn zsh_hook_dir() -> anyhow::Result<std::path::PathBuf> {
    use std::{
        io::Write,
        os::unix::fs::{DirBuilderExt, OpenOptionsExt},
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let base = std::env::temp_dir();
    let mut attempt = 0;
    let dir = loop {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.subsec_nanos());
        let name = format!(
            "{ZSH_HOOK_PREFIX}{}-{}-{nanos:x}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = base.join(name);
        // mkdir fails on an existing name, so no one else's folder is used.
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => break dir,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt < 8 => {
                attempt += 1;
            }
            Err(error) => return Err(error.into()),
        }
    };
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dir.join(".zshenv"))
        .and_then(|mut file| file.write_all(zsh_env().as_bytes()));
    if let Err(error) = written {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(error.into());
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_cover_every_command_and_ask_the_registry_for_names() {
        for shell in ["powershell", "bash", "zsh"] {
            let hook = script(shell).unwrap();
            assert!(hook.contains("cmd --complete") && hook.contains("clear-error"));
            assert!(!hook.contains('\r'));
        }
        assert!(script("fish").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn powershell_tabs_register_the_completer_after_the_prompt_hook() {
        let profile = [
            crate::ShellProfileId::Pwsh,
            crate::ShellProfileId::WindowsPowerShell,
        ]
        .into_iter()
        .find_map(|id| crate::shell_profile(id).ok())
        .expect("Windows PowerShell is installed");
        let mut command = profile.command().unwrap();
        let listed = command.args.clone();
        add_powershell_completion(&mut command);
        let script = command.args.last().unwrap().to_str().unwrap();
        assert_eq!(command.args.len(), listed.len());
        assert!(script.starts_with(include_str!("powershell-init.ps1")));
        assert!(script.ends_with(POWERSHELL));
        // The listed profile stays within the wire limit for one argument.
        assert!(listed.iter().all(|arg| arg.len() <= 4096));
        // Windows command-line quoting passes the script through unchanged.
        assert!(!script.contains('"'));
    }

    #[test]
    fn the_bash_hook_keeps_the_status_and_an_inherited_prompt_command() {
        let value = bash_prompt_command(true, None).into_string().unwrap();
        assert!(value.starts_with("__shellfox_status=$?\n"));
        assert!(value.contains("complete -p shellfox &>/dev/null ||"));
        assert!(value.contains("complete -F _shellfox_complete shellfox"));
        assert!(value.contains("shellfox_status=$__shellfox_status"));
        assert!(value.ends_with(include_str!("bash-prompt.sh").trim_end()));
        assert!(!value.contains("@COMPLETION@"));
        assert!(!value.lines().any(|line| line.trim_start().starts_with('#')));
        assert_eq!(bash_prompt_command(true, Some("".as_ref())), value.as_str());
        assert_eq!(
            bash_prompt_command(true, Some("history -a".as_ref())),
            format!("{value}\nhistory -a").as_str()
        );
        let wsl = bash_prompt_command(false, None).into_string().unwrap();
        assert!(wsl.ends_with("(exit \"$__shellfox_status\")") && !wsl.contains("]7;"));
    }

    #[test]
    fn the_zsh_hook_restores_zdotdir_before_sourcing_user_files() {
        let env = zsh_env();
        assert!(env.contains(ZSH));
        let restore = env.find("ZDOTDIR=$SHELLFOX_ZDOTDIR").unwrap();
        let unset = env.find("'unset' ZDOTDIR").unwrap();
        let source = env.find("'source'").unwrap();
        assert!(restore < source && unset < source);
        assert!(env.contains(ZSH_SAVED_ZDOTDIR) && env.contains(ZSH_HOOK_PREFIX));
        assert!(env.contains("(( $+_comps[shellfox] )) && return 0"));
    }
}
