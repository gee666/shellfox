//! `shellfox completion <powershell|bash|zsh>`: the 0.9.15 hooks, byte for byte.
//! Shellfox tabs load them by themselves (shellfox_terminal::completion).
use anyhow::Result;
use shellfox_terminal::completion::script;

pub const HELP: &str = include_str!("completion/help.txt");

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
