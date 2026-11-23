//! `shellfox completion <powershell|bash|zsh>`: the 0.9.15 hooks, byte for byte.
//! They only list names; `cmd --complete` supplies registered command names.
use anyhow::Result;

pub const HELP: &str = include_str!("completion/help.txt");

pub fn script(shell: &str) -> Option<&'static str> {
    match shell {
        "powershell" => Some(include_str!("completion/powershell.txt")),
        "bash" => Some(include_str!("completion/bash.txt")),
        "zsh" => Some(include_str!("completion/zsh.txt")),
        _ => None,
    }
}

/// 0.9 exit codes: 0 for a hook or the usage, 2 for an unknown shell.
pub fn run(shell: Option<&str>) -> Result<i32> {
    let json = crate::output::mode() == crate::output::Mode::Json;
    match shell.map(script) {
        None if !json => crate::output::print_text(HELP)?,
        Some(Some(script)) => crate::output::print_text(script)?,
        _ if json => return Err(crate::output::CliError::usage(HELP.trim())),
        _ => {
            eprint!("{HELP}");
            return Ok(2);
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hooks_cover_every_command_and_ask_the_registry_for_names() {
        for shell in ["powershell", "bash", "zsh"] {
            let hook = script(shell).unwrap();
            assert!(hook.contains("cmd --complete") && hook.contains("clear-error"));
            assert!(!hook.contains('\r'));
        }
        assert!(script("fish").is_none());
    }
}
