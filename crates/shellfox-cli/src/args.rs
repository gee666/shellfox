use anyhow::{Context, Result, bail};
use shellfox_core::{
    AddTab, CreateSession, ProfileId, SessionId, SettingsPatch, TabId, validation,
};
use shellfox_service::experiment::{ExperimentRequest, SettingsRequest};
use std::{collections::HashMap, ffi::OsString, path::PathBuf};

#[path = "args_help.rs"]
mod help;
pub use help::HELP;

#[derive(Debug)]
pub struct Args {
    pub data_dir: PathBuf,
    pub helper: PathBuf,
    pub helper_sha256: [u8; 32],
    pub command: Command,
    pub application: crate::app_composition::NativeApplicationConfig,
}

#[derive(Debug)]
pub enum Command {
    Serve,
    Power(shellfox_service::power::PowerRequest),
    Network(crate::remote_args::NetworkCommand),
    Sessions,
    Tabs(Option<SessionId>),
    Request(ExperimentRequest),
    Attach {
        tab: TabId,
        takeover: bool,
    },
    DesktopLaunch(shellfox_service::desktop_launch::DesktopLaunchIntent),
    Automation(crate::automation_args::AutomationCommand),
    #[cfg(feature = "experimental-shellfox-native")]
    Peer(crate::peer_args::PeerArgs),
}

/// Paths retain their native encoding. Command arguments are UTF-8 metadata.
pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Option<Args>> {
    let mut args = arguments.into_iter().peekable();
    if args
        .peek()
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        args.next();
        if args.next().is_some() {
            bail!("--help does not take other arguments.");
        }
        return Ok(None);
    }
    let mut data_dir = None;
    let mut helper = None;
    let mut helper_sha256 = None;
    let mut application = crate::app_composition::ApplicationArguments::default();
    while let Some(flag) = args.peek() {
        if crate::app_composition::ApplicationArguments::is_flag(flag) {
            let flag = args.next().context("Missing application flag")?;
            let value = args.next().context("Application flag requires a value")?;
            application.push(
                flag.to_str().context("Application flag must be UTF-8")?,
                value,
            )?;
            continue;
        }
        if flag == "--helper-sha256" {
            args.next();
            if helper_sha256.is_some() {
                bail!("Duplicate flag --helper-sha256.");
            }
            let value = args.next().context("--helper-sha256 requires a value.")?;
            helper_sha256 = Some(parse_digest(
                value.to_str().context("Invalid SHA-256 encoding.")?,
            )?);
            continue;
        }
        let target = if flag == "--data-dir" {
            &mut data_dir
        } else if flag == "--helper" {
            &mut helper
        } else {
            break;
        };
        let flag = args.next().context("Missing path flag.")?;
        if target.is_some() {
            bail!("Duplicate flag {}.", flag.to_string_lossy());
        }
        let path = PathBuf::from(args.next().context("Path flag requires a value.")?);
        if !path.is_absolute() {
            bail!("{} requires an absolute path.", flag.to_string_lossy());
        }
        *target = Some(path);
    }
    let data_dir =
        data_dir.context("Explicit --data-dir ABSOLUTE_DIR required. Use --help for usage.")?;
    let helper =
        helper.context("Explicit --helper ABSOLUTE_EXE required. Use --help for usage.")?;
    let helper_sha256 = helper_sha256
        .context("Explicit --helper-sha256 HEX required. Use a trusted build digest.")?;
    let words: Vec<String> = args
        .map(|arg| {
            arg.into_string()
                .map_err(|_| anyhow::anyhow!("Command arguments must be valid UTF-8."))
        })
        .collect::<Result<_>>()?;
    let command = parse_command(&words)?;
    let application = application.finish_with_helper(&data_dir, &helper, helper_sha256)?;
    application.validate_separate(&[&helper])?;
    Ok(Some(Args {
        data_dir,
        helper,
        helper_sha256,
        command,
        application,
    }))
}

fn parse_digest(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("--helper-sha256 requires exactly 64 hex digits from a trusted build.");
    }
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)?;
    }
    Ok(digest)
}

fn parse_command(words: &[String]) -> Result<Command> {
    if words.first().is_some_and(|word| word == "peer") {
        #[cfg(not(feature = "experimental-shellfox-native"))]
        bail!("peer requires experimental-shellfox-native; no host connection was attempted.");
        #[cfg(feature = "experimental-shellfox-native")]
        {
            let peer_words: Vec<OsString> = words.iter().map(OsString::from).collect();
            return crate::peer_args::parse_peer(&peer_words)?
                .map(Command::Peer)
                .ok_or_else(|| anyhow::anyhow!("Invalid peer command."));
        }
    }
    let parts: Vec<&str> = words.iter().map(String::as_str).collect();
    if let Some(command) = crate::remote_args::parse(&parts)? {
        return Ok(Command::Network(command));
    }
    if let Some(intent) = crate::launch_args::parse(&parts)? {
        return Ok(Command::DesktopLaunch(intent));
    }
    if let Some(command) = crate::automation_args::parse(&parts)? {
        return Ok(Command::Automation(command));
    }
    let request = match parts.as_slice() {
        ["host", "serve"] => return Ok(Command::Serve),
        ["host", "power", "status"] => {
            return Ok(Command::Power(
                shellfox_service::power::PowerRequest::Status {},
            ));
        }
        ["host", "power", "on"] => {
            return Ok(Command::Power(shellfox_service::power::PowerRequest::Set {
                active: true,
            }));
        }
        ["host", "power", "off"] => {
            return Ok(Command::Power(shellfox_service::power::PowerRequest::Set {
                active: false,
            }));
        }
        ["host", "stop"] => ExperimentRequest::Stop {},
        ["session", "list"] => return Ok(Command::Sessions),
        ["tab", "list"] => return Ok(Command::Tabs(None)),
        ["tab", "list", id] => return Ok(Command::Tabs(Some(id.parse()?))),
        ["session", "create", options @ ..] => {
            let mut options = Options::parse(options, &["--cwd", "--profile", "--title"])?;
            let cwd = options.required("--cwd")?;
            validation::cwd(&cwd)?;
            ExperimentRequest::CreateSession {
                input: CreateSession {
                    request_id: SessionId::new(),
                    title: options.title()?,
                    cwd,
                    profile_id: options.profile()?,
                },
            }
        }
        ["tab", "create", id, options @ ..] => {
            let id = id.parse()?;
            let mut options = Options::parse(options, &["--cwd", "--profile", "--title"])?;
            let cwd = options.take("--cwd");
            if let Some(cwd) = &cwd {
                validation::cwd(cwd)?;
            }
            ExperimentRequest::AddTab {
                id,
                input: AddTab {
                    title: options.title()?,
                    cwd,
                    profile_id: options.profile()?,
                },
            }
        }
        ["session", "rename", id, title] => {
            validation::title(title)?;
            ExperimentRequest::RenameSession {
                id: id.parse()?,
                title: (*title).into(),
            }
        }
        ["session", "close", id] => ExperimentRequest::DeleteSession { id: id.parse()? },
        ["tab", "rename", id, title] => {
            validation::title(title)?;
            ExperimentRequest::RenameTab {
                id: id.parse()?,
                title: (*title).into(),
            }
        }
        ["tab", "close", id] => ExperimentRequest::CloseTab { id: id.parse()? },
        ["settings", "get"] => ExperimentRequest::Settings {
            request: SettingsRequest::Get {},
        },
        ["settings", "set", json] => {
            if json.len() > 1024 * 1024 {
                bail!("Settings patch exceeds 1 MiB.");
            }
            let value: serde_json::Value =
                serde_json::from_str(json).context("Invalid settings patch JSON.")?;
            if !value.is_object() {
                bail!("Settings patch must be a JSON object.");
            }
            let patch: SettingsPatch =
                serde_json::from_value(value).context("Invalid settings patch JSON.")?;
            let request = SettingsRequest::Save(Box::new(patch));
            shellfox_service::native::NativeAction::Settings(request.clone()).validate()?;
            ExperimentRequest::Settings { request }
        }
        ["attach", id] => {
            return Ok(Command::Attach {
                tab: id.parse()?,
                takeover: false,
            });
        }
        ["attach", id, "--takeover"] => {
            return Ok(Command::Attach {
                tab: id.parse()?,
                takeover: true,
            });
        }
        _ => bail!("Unknown command, flags, or argument count. Use --help for usage."),
    };
    Ok(Command::Request(request))
}

struct Options<'a>(HashMap<&'a str, &'a str>);
impl<'a> Options<'a> {
    fn parse(words: &[&'a str], allowed: &[&str]) -> Result<Self> {
        let mut options = HashMap::new();
        let mut pairs = words.chunks_exact(2);
        for pair in &mut pairs {
            if !allowed.contains(&pair[0]) {
                bail!("Unknown option {}.", pair[0]);
            }
            if options.insert(pair[0], pair[1]).is_some() {
                bail!("Duplicate option {}.", pair[0]);
            }
        }
        if !pairs.remainder().is_empty() {
            bail!("Option requires a value.");
        }
        Ok(Self(options))
    }
    fn take(&mut self, name: &str) -> Option<String> {
        self.0.remove(name).map(str::to_owned)
    }
    fn required(&mut self, name: &str) -> Result<String> {
        self.take(name)
            .with_context(|| format!("Required option {name} is missing."))
    }
    fn title(&mut self) -> Result<Option<String>> {
        let title = self.take("--title");
        if let Some(title) = &title {
            validation::title(title)?;
        }
        Ok(title)
    }
    fn profile(&mut self) -> Result<ProfileId> {
        Ok(self.required("--profile")?.parse()?)
    }
}

#[cfg(test)]
#[path = "args_application_tests.rs"]
mod application_tests;

#[cfg(all(test, not(feature = "experimental-shellfox-native")))]
mod default_peer_availability {
    use super::*;
    #[test]
    fn peer_is_rejected_during_argument_dispatch_before_any_host_connection() {
        for words in [vec!["peer"], vec!["peer", "n-work", "sessions"]] {
            let words: Vec<String> = words.into_iter().map(str::to_owned).collect();
            assert!(
                matches!(parse_command(&words),Err(error) if error.to_string().contains("experimental-shellfox-native"))
            );
        }
    }
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
