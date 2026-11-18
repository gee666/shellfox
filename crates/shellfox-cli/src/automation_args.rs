use anyhow::{bail, Context, Result};
use shellfox_service::local::automation::AutomationStart;
use std::{collections::BTreeMap, fmt};
use uuid::Uuid;

pub enum AutomationCommand {
    Run(Box<AutomationStart>),
    List,
    Show(Uuid),
    Stop(Uuid),
}
// Command-line Debug must never include target arguments or environment values.
impl fmt::Debug for AutomationCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AutomationCommand(<redacted>)")
    }
}
pub fn parse(words: &[&str]) -> Result<Option<AutomationCommand>> {
    match words {
        ["runs", "list"] | ["runs"] => return Ok(Some(AutomationCommand::List)),
        ["runs", "show", id] => return Ok(Some(AutomationCommand::Show(id.parse()?))),
        ["runs", "stop", id] => return Ok(Some(AutomationCommand::Stop(id.parse()?))),
        ["run", rest @ ..] => return Ok(Some(AutomationCommand::Run(Box::new(run(rest)?)))),
        ["runs", ..] => bail!("Use runs list, runs show RUN_ID or runs stop RUN_ID."),
        _ => {}
    }
    Ok(None)
}
fn run(words: &[&str]) -> Result<AutomationStart> {
    let mut cwd = None;
    let mut title = None;
    let mut request_id = None;
    let mut detached = false;
    let mut index = 0;
    while let Some(flag) = words.get(index) {
        match *flag {
            "--" => {
                index += 1;
                break;
            }
            "--detach" if !detached => detached = true,
            "--cwd" | "--title" | "--request-id" => {
                let value = words
                    .get(index + 1)
                    .context("Run option requires a value.")?;
                match *flag {
                    "--cwd" if cwd.is_none() => cwd = Some(*value),
                    "--title" if title.is_none() => title = Some((*value).to_owned()),
                    "--request-id" if request_id.is_none() => request_id = Some(value.parse()?),
                    _ => bail!("Duplicate run option."),
                }
                index += 1;
            }
            _ => {
                bail!("Unknown or duplicate run option. Use run [OPTIONS] -- EXECUTABLE [ARG...].")
            }
        }
        index += 1;
    }
    if index == 0 || words.get(index.wrapping_sub(1)) != Some(&"--") || index >= words.len() {
        bail!("Use run [OPTIONS] -- EXECUTABLE [ARG...].");
    }
    let invoking = std::env::current_dir()?;
    let cwd = crate::launch_args::directory(cwd.unwrap_or("."), &invoking)?;
    Ok(AutomationStart {
        request_id: request_id.unwrap_or_else(Uuid::new_v4),
        executable: words[index].to_owned(),
        args: words[index + 1..]
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect(),
        cwd,
        env: environment(std::env::vars_os())?,
        title,
        detached,
    })
}
fn environment(
    values: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Result<BTreeMap<String, String>> {
    values
        .into_iter()
        .map(|(key, value)| {
            // The JSON contract is Unicode. Reject unrepresentable environment data
            // instead of replacing bytes, panicking, or silently dropping variables.
            Ok((
                key.into_string().map_err(|_| {
                    anyhow::anyhow!("Environment name is not representable by the local protocol.")
                })?,
                value.into_string().map_err(|_| {
                    anyhow::anyhow!("Environment value is not representable by the local protocol.")
                })?,
            ))
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn argv_remains_literal_and_empty_arguments_survive() {
        let Some(AutomationCommand::Run(spec)) = parse(&[
            "run", "--detach", "--", "program", "", "a b", "$HOME", "&", "--flag",
        ])
        .unwrap() else {
            panic!("missing run");
        };
        assert_eq!(spec.executable, "program");
        assert_eq!(spec.args, ["", "a b", "$HOME", "&", "--flag"]);
        assert!(spec.detached);
        assert!(!format!("{:?}", AutomationCommand::Run(spec)).contains("$HOME"));
    }
    #[test]
    fn separator_and_duplicate_flags_are_rejected() {
        for words in [
            &["run"][..],
            &["run", "program"],
            &["run", "--"],
            &["run", "--detach", "--detach", "--", "program"],
            &["run", "--cwd", ".", "--cwd", ".", "--", "program"],
        ] {
            assert!(parse(words).is_err());
        }
    }
    #[test]
    fn environment_is_not_rewritten() {
        let values = [
            ("NAME".into(), " a= b\n".into()),
            ("EMPTY".into(), "".into()),
            ("ELECTRON_RUN_AS_NODE".into(), "1".into()),
        ];
        let env = environment(values).unwrap();
        assert_eq!(env["NAME"], " a= b\n");
        assert_eq!(env["EMPTY"], "");
        assert_eq!(env["ELECTRON_RUN_AS_NODE"], "1");
    }
}
