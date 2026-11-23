//! `shellfox cmd`/`commands`: saved script references, from 0.9.15's
//! shellfox-cli-commands.ts and shellfox-cli-dispatch.ts. The registry is the
//! same `<user data>/cli-commands.json`, so 0.9 registrations keep working.
use crate::output::{print_text, CliError};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[path = "custom_scripts.rs"]
mod scripts;
pub use scripts::Location;

pub const HELP: &str = "Custom commands:
  shellfox cmd --register <name> <script-path>
  shellfox cmd --ls | --list
  shellfox cmd --remove <name> | --rm <name> | --unregister <name>
  shellfox cmd --rename <old-name> <new-name>
  shellfox cmd <name> [args...]
  shellfox cmd --run <name> [args...]
  shellfox cmd --help
Use shellfox completion <powershell|bash|zsh> for Tab completion.
Paths are references. Each invocation reads the current script.
All arguments after the name go to the script unchanged.
";

/// Words each administration operation consumes, as 0.9's global parser counted them.
pub fn operand_count(operation: &str) -> Option<usize> {
    Some(match operation {
        "--register" | "--rename" => 2,
        "--remove" | "--rm" | "--unregister" | "--complete" => 1,
        "--ls" | "--list" | "--help" | "-h" => 0,
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub name: String,
    #[serde(flatten)]
    pub location: Location,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    version: u8,
    commands: Vec<Command>,
}

fn key(name: &str) -> String {
    name.to_ascii_lowercase()
}
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
fn check_name(name: &str) -> Result<()> {
    if !valid_name(name) {
        bail!("Command names must start with a letter and contain only letters, digits, _ or -, up to 64 characters.");
    }
    Ok(())
}
fn validate(commands: &[Command]) -> Result<()> {
    let mut names = std::collections::HashSet::new();
    for command in commands {
        if !valid_name(&command.name)
            || !command.location.valid()
            || !names.insert(key(&command.name))
        {
            bail!("Invalid custom command registry.");
        }
    }
    Ok(())
}

/// JSON-quoted text with C1 controls escaped, as 0.9 `display` showed names and paths.
fn display(text: &str) -> String {
    crate::terminal_text::escape_json_display(&serde_json::to_string(text).unwrap_or_default())
}

struct Store(PathBuf);
impl Store {
    fn file(&self) -> PathBuf {
        self.0.join("cli-commands.json")
    }
    fn read(&self) -> Result<Vec<Command>> {
        let bytes = match std::fs::read(self.file()) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let registry: Registry = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Invalid custom command registry."))?;
        if registry.version != 1 {
            bail!("Invalid custom command registry.");
        }
        validate(&registry.commands)?;
        Ok(registry.commands)
    }
    /// One writer at a time through an exclusive lock file; the new registry replaces the old atomically.
    fn mutate(&self, change: impl FnOnce(&mut Vec<Command>) -> Result<()>) -> Result<()> {
        std::fs::create_dir_all(&self.0)?;
        let lock = self.0.join("cli-commands.json.lock");
        let mut held = None;
        for _ in 0..100 {
            match private_file(&lock) {
                Ok(file) => {
                    held = Some(file);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(error) => return Err(error.into()),
            }
        }
        if held.is_none() {
            bail!("Custom command registry is locked. Retry, or remove cli-commands.json.lock after checking no registration is running.");
        }
        let next = self
            .0
            .join(format!("cli-commands.json.{}.new", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut commands = self.read()?;
            change(&mut commands)?;
            let mut output = private_file(&next)?;
            let text = serde_json::to_string_pretty(&Registry {
                version: 1,
                commands,
            })?;
            output.write_all(format!("{text}\n").as_bytes())?;
            output.sync_all()?;
            drop(output);
            std::fs::rename(&next, self.file())?;
            Ok(())
        })();
        let _ = std::fs::remove_file(&next);
        drop(held);
        let _ = std::fs::remove_file(&lock);
        result
    }
    fn find(&self, name: &str) -> Result<Command> {
        self.read()?
            .into_iter()
            .find(|command| key(&command.name) == key(name))
            .ok_or_else(|| anyhow::anyhow!("No custom command named {name}"))
    }
}
fn private_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// A usage mistake (exit 2), as opposed to a failed operation (exit 1).
#[derive(Debug)]
struct Usage(String);
impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Usage {}
fn usage(text: impl Into<String>) -> anyhow::Error {
    Usage(text.into()).into()
}

pub fn run(words: &[String], user_data: &Path, json: bool) -> Result<i32> {
    let operation = words.first().map(String::as_str);
    let admin = operation.is_none_or(|op| operand_count(op).is_some());
    if json && !admin {
        return Err(CliError::usage(
            "Custom script execution does not support --json.",
        ));
    }
    let store = Store(user_data.to_owned());
    let mut output = String::new();
    match dispatch(&store, words, &mut output) {
        Ok(Some(code)) => Ok(code),
        Ok(None) if json => print_text(output.trim_end()).map(|()| 0),
        Ok(None) => {
            let color = operation != Some("--complete") && crate::human_text::color(true);
            let text = if color {
                crate::human_text::Style(true).accent(&output)
            } else {
                output
            };
            print_text(&text).map(|()| 0)
        }
        Err(error) if json => Err(match error.downcast_ref::<Usage>() {
            Some(usage) => CliError::usage(format!("Shellfox: {}", display(&usage.0))),
            None => CliError::new(
                "COMMAND_FAILED",
                format!("Shellfox: {}", display(&error.to_string())),
                false,
            ),
        }),
        Err(error) => {
            let style = crate::human_text::Style(crate::human_text::color(false));
            eprintln!(
                "{}{}",
                style.red("Shellfox: "),
                display(&format!("{error:#}"))
            );
            Ok(if error.is::<Usage>() { 2 } else { 1 })
        }
    }
}

/// Some(code) after running a script; None when `output` holds the text to print.
fn dispatch(store: &Store, words: &[String], output: &mut String) -> Result<Option<i32>> {
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] | ["--help" | "-h"] => output.push_str(HELP),
        ["--help" | "-h", ..] => return Err(usage("Usage: shellfox cmd --help")),
        ["--complete", prefix] => {
            let mut names: Vec<String> = store
                .read()?
                .into_iter()
                .map(|c| c.name)
                .filter(|name| key(name).starts_with(&key(prefix)))
                .collect();
            names.sort_by_key(|name| key(name));
            if !names.is_empty() {
                output.push_str(&format!("{}\n", names.join("\n")));
            }
        }
        ["--complete", ..] => return Err(usage("Usage: shellfox cmd --complete <prefix>")),
        ["--register", name, path] => {
            check_name(name)?;
            let location = scripts::resolve(path)?;
            store.mutate(|commands| {
                if commands.iter().any(|c| key(&c.name) == key(name)) {
                    bail!("Command already registered: {name}");
                }
                commands.push(Command {
                    name: (*name).to_owned(),
                    location,
                });
                validate(commands)
            })?;
            output.push_str(&format!("Registered {}.\n", display(name)));
        }
        ["--register", ..] => {
            return Err(usage("Usage: shellfox cmd --register <name> <script-path>"))
        }
        [op @ ("--remove" | "--rm" | "--unregister"), name] => {
            let _ = op;
            store.mutate(|commands| {
                let index = commands
                    .iter()
                    .position(|c| key(&c.name) == key(name))
                    .ok_or_else(|| anyhow::anyhow!("No custom command named {name}"))?;
                commands.remove(index);
                Ok(())
            })?;
            output.push_str(&format!("Removed {}.\n", display(name)));
        }
        [op @ ("--remove" | "--rm" | "--unregister"), ..] => {
            return Err(usage(format!("Usage: shellfox cmd {op} <name>")))
        }
        ["--rename", old, new] => {
            check_name(new)?;
            store.mutate(|commands| {
                if commands
                    .iter()
                    .any(|c| key(&c.name) == key(new) && key(&c.name) != key(old))
                {
                    bail!("Command already registered: {new}");
                }
                let command = commands
                    .iter_mut()
                    .find(|c| key(&c.name) == key(old))
                    .ok_or_else(|| anyhow::anyhow!("No custom command named {old}"))?;
                command.name = (*new).to_owned();
                Ok(())
            })?;
            output.push_str(&format!("Renamed {} to {}.\n", display(old), display(new)));
        }
        ["--rename", ..] => {
            return Err(usage("Usage: shellfox cmd --rename <old-name> <new-name>"))
        }
        [op @ ("--ls" | "--list")] => {
            let _ = op;
            let mut commands = store.read()?;
            commands.sort_by_key(|c| key(&c.name));
            if commands.is_empty() {
                output.push_str("No custom commands.\n");
            } else {
                let wsl = commands.iter().any(|c| c.location.distro.is_some());
                let mut headers = vec!["NAME".to_owned(), "PATH".to_owned()];
                if wsl {
                    headers.push("WSL".into());
                }
                let rows: Vec<Vec<String>> = commands
                    .iter()
                    .map(|c| {
                        let mut row = vec![c.name.clone(), display(&c.location.path)];
                        if wsl {
                            row.push(
                                c.location
                                    .distro
                                    .as_deref()
                                    .map_or_else(|| "none".into(), display),
                            );
                        }
                        row
                    })
                    .collect();
                let table = crate::human_text::table(
                    &headers,
                    &rows,
                    crate::human_text::Style(false),
                    crate::human_text::terminal_width(),
                    &|v| v.to_owned(),
                );
                output.push_str(&format!("{table}\n"));
            }
        }
        [op @ ("--ls" | "--list"), ..] => return Err(usage(format!("Usage: shellfox cmd {op}"))),
        rest => {
            let (name, args) = match rest {
                ["--run", name, args @ ..] => (*name, args),
                [name, args @ ..] => (*name, args),
                [] => unreachable!("help handled above"),
            };
            if name == "--run" || name.starts_with('-') {
                return Err(usage("Usage: shellfox cmd [--run] <name> [args...]"));
            }
            check_name(name)?;
            let command = store.find(name)?;
            let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
            return scripts::execute(&command.location, &args).map(Some);
        }
    }
    Ok(None)
}

#[cfg(test)]
#[path = "custom_commands_tests.rs"]
mod tests;
