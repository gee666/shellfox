use crate::output::CliError;
use anyhow::Result;
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
const RUN_USAGE: &str =
    "Usage: shellfox run [--cwd PATH] [--title TITLE] [--detach] -- EXECUTABLE [ARG...]";
const RUNS_USAGE: &str = "Usage: shellfox runs list|show RUN_UUID|stop RUN_UUID";

pub fn parse(words: &[&str]) -> Result<Option<AutomationCommand>> {
    Ok(Some(match words {
        ["run", rest @ ..] => AutomationCommand::Run(Box::new(run(rest)?)),
        ["runs"] | ["runs", "list"] => AutomationCommand::List,
        ["runs", "list", ..] => return Err(positionals(0)),
        ["runs", "show", id] => AutomationCommand::Show(run_id(id)?),
        ["runs", "stop", id] => AutomationCommand::Stop(run_id(id)?),
        ["runs", "show" | "stop", ..] => return Err(positionals(1)),
        ["runs", ..] => return Err(CliError::usage(RUNS_USAGE)),
        _ => return Ok(None),
    }))
}
fn positionals(count: usize) -> anyhow::Error {
    CliError::usage(format!("Expected {count} positional arguments."))
}
/// 0.9 took only the full hyphenated form.
fn full_uuid(id: &str) -> Option<Uuid> {
    (id.len() == 36).then(|| Uuid::try_parse(id).ok()).flatten()
}
fn run_id(id: &str) -> Result<Uuid> {
    full_uuid(id).ok_or_else(|| CliError::usage("Expected a full run UUID."))
}
/// 0.9's rules: flags before `--`, each once, as `--flag VALUE` or `--flag=VALUE`.
fn run(words: &[&str]) -> Result<AutomationStart> {
    let separator = words
        .iter()
        .position(|word| *word == "--")
        .filter(|at| words.get(at + 1).is_some_and(|word| !word.is_empty()))
        .ok_or_else(|| CliError::usage(RUN_USAGE))?;
    let mut values: BTreeMap<&str, &str> = BTreeMap::new();
    let mut detached = false;
    let mut extra = false;
    let mut flags = words[..separator].iter();
    while let Some(word) = flags.next() {
        if !word.starts_with("--") {
            extra = true;
            continue;
        }
        let (flag, inline) = match word.split_once('=') {
            Some((flag, value)) => (flag, Some(value)),
            None => (*word, None),
        };
        if values.contains_key(flag) || (flag == "--detach" && detached) {
            return Err(CliError::usage(format!("Duplicate option: {flag}")));
        }
        match flag {
            "--detach" if inline.is_some() => {
                return Err(CliError::usage("--detach does not accept a value."))
            }
            "--detach" => detached = true,
            "--cwd" | "--title" | "--request-id" => {
                let value = inline
                    .or_else(|| flags.next().copied())
                    .ok_or_else(|| CliError::usage(format!("Missing value for {flag}")))?;
                values.insert(flag, value);
            }
            _ => return Err(CliError::usage(format!("Unknown option: {flag}"))),
        }
    }
    if extra {
        return Err(positionals(0));
    }
    let request_id = match values.get("--request-id") {
        Some(id) => full_uuid(id).ok_or_else(|| CliError::usage("--request-id must be a UUID."))?,
        None => Uuid::new_v4(),
    };
    let invoking = std::env::current_dir()?;
    let cwd = values.get("--cwd").copied().unwrap_or(".");
    let cwd = crate::launch_args::directory(cwd, &invoking)?;
    Ok(AutomationStart {
        request_id,
        executable: words[separator + 1].to_owned(),
        args: words[separator + 2..]
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect(),
        cwd,
        env: environment(std::env::vars_os())?,
        title: values.get("--title").map(|title| (*title).to_owned()),
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
    fn usage(words: &[&str]) -> String {
        let error = parse(words).expect_err("usage error");
        let cli = error.downcast_ref::<CliError>().expect("CliError");
        assert_eq!(cli.code, "VALIDATION");
        cli.message.clone()
    }
    #[test]
    fn argument_errors_are_usage_errors_with_0_9_messages() {
        assert_eq!(
            usage(&["runs", "show", "nope"]),
            "Expected a full run UUID."
        );
        assert_eq!(
            usage(&["runs", "stop", "{67e55044-10b1-426f-9247-bb680e5fe0c8}"]),
            "Expected a full run UUID."
        );
        assert_eq!(usage(&["runs", "show"]), "Expected 1 positional arguments.");
        assert!(usage(&["runs", "nope"]).starts_with("Usage: shellfox runs"));
        assert!(usage(&["run", "program"]).starts_with("Usage: shellfox run"));
        assert_eq!(
            usage(&["run", "--request-id", "x", "--", "p"]),
            "--request-id must be a UUID."
        );
        assert_eq!(
            usage(&["run", "--cwd=.", "--cwd", ".", "--", "p"]),
            "Duplicate option: --cwd"
        );
        assert_eq!(
            usage(&["run", "--cwd", "--", "p"]),
            "Missing value for --cwd"
        );
        assert_eq!(
            usage(&["run", "--nope", "--", "p"]),
            "Unknown option: --nope"
        );
        assert_eq!(
            usage(&["run", "x", "--", "p"]),
            "Expected 0 positional arguments."
        );
        let id = "67e55044-10b1-426f-9247-bb680e5fe0c8";
        assert!(matches!(
            parse(&["runs", "show", id]).unwrap(),
            Some(AutomationCommand::Show(run)) if run.to_string() == id
        ));
        let Some(AutomationCommand::Run(spec)) =
            parse(&["run", "--title=T", "--request-id", id, "--", "p"]).unwrap()
        else {
            panic!("missing run");
        };
        assert_eq!(spec.title.as_deref(), Some("T"));
        assert_eq!(spec.request_id.to_string(), id);
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
