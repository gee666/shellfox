//! Standalone SSH runs in the caller's terminal, not in a host-owned terminal tab.
#[path = "ssh_auth.rs"]
mod auth;
#[path = "ssh_io.rs"]
mod io;
#[path = "ssh_picker.rs"]
mod picker;
#[path = "ssh_session.rs"]
mod session;
#[path = "ssh_terminal.rs"]
mod terminal;

use anyhow::{bail, ensure, Context, Result};
use picker::escape;
use shellfox_service::{
    app_paths,
    local::{self, LocalClient, SshRead, SshReadReply},
};
use shellfox_ssh::{PrivateDirectory, ProfileDto};
use std::{ffi::OsString, path::PathBuf, sync::Arc};
use terminal::Terminal;

#[derive(Debug, PartialEq)]
enum Action {
    Choose,
    Profile(String),
    Help,
}

fn action(args: &[OsString]) -> Option<Action> {
    let name = |name: &OsString| {
        let name = name.to_str()?;
        (!name.is_empty() && !name.starts_with('-')).then(|| Action::Profile(name.to_owned()))
    };
    match args {
        [] => Some(Action::Choose),
        [arg] if arg == "-h" || arg == "--help" || arg == "help" => Some(Action::Help),
        [arg] => name(arg),
        [flag, arg] if flag == "--profile" || flag == "-p" => name(arg),
        _ => None,
    }
}

#[derive(Default)]
struct Options {
    data: Option<PathBuf>,
    helper: Option<PathBuf>,
    digest: Option<OsString>,
    incompatible: bool,
}

/// Parse SSH before host-only startup arguments and helper checks. `ssh --help`
/// and an empty offline store must work even on an uninstalled build.
pub fn dispatch(arguments: &[OsString]) -> Option<Result<i32>> {
    let mut opts = Options::default();
    let mut rest = arguments;
    loop {
        let [flag, tail @ ..] = rest else { return None };
        if flag == "ssh" {
            rest = tail;
            break;
        }
        if flag == "--json" {
            opts.incompatible = true;
            rest = tail;
            continue;
        }
        let [value, remaining @ ..] = tail else {
            return None;
        };
        match flag.to_str()? {
            "--data-dir" | "--user-data" => opts.data = Some(PathBuf::from(value)),
            "--helper" => opts.helper = Some(PathBuf::from(value)),
            "--helper-sha256" => opts.digest = Some(value.clone()),
            "--host" => opts.incompatible |= value != "local",
            _ => return None,
        }
        rest = remaining;
    }
    let mut args = Vec::new();
    while let [flag, tail @ ..] = rest {
        if flag == "--json" {
            opts.incompatible = true;
            rest = tail;
        } else if flag == "--host" {
            let [host, next @ ..] = tail else {
                return Some(Ok(2));
            };
            opts.incompatible |= host != "local";
            rest = next;
        } else {
            args.push(flag.clone());
            rest = tail;
        }
    }
    Some(run(opts, action(&args)))
}

fn run(opts: Options, action: Option<Action>) -> Result<i32> {
    let terminal = Terminal::new();
    if opts.incompatible {
        eprintln!("SSH is interactive and local. It does not support --json or a remote --host.");
        return Ok(2);
    }
    let Some(action) = action else {
        terminal.line(&terminal.style("31", "invalid ssh arguments"))?;
        terminal.line(&terminal.dim("run shellfox --help"))?;
        return Ok(2);
    };
    if action == Action::Help {
        terminal.help(true)?;
        return Ok(0);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        tokio::select! {
            result = connect(opts, &terminal, action) => result,
            signal = cancellation_signal() => { signal?; Err(terminal::Cancelled.into()) }
        }
    });
    match result {
        Ok(code) => Ok(code),
        Err(error) if error.is::<terminal::Cancelled>() => Ok(130),
        Err(error) => {
            terminal.line(&terminal.style("31", &escape(&error.to_string())))?;
            Ok(if error.is::<session::ConnectionError>() {
                255
            } else {
                1
            })
        }
    }
}

async fn cancellation_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut terminate = signal(SignalKind::terminate())?;
        let mut hangup = signal(SignalKind::hangup())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
            _ = hangup.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
}

async fn connect(opts: Options, terminal: &Terminal, action: Action) -> Result<i32> {
    let store = Store::open(opts).await?;
    let mut profiles = store.list().await?;
    profiles.sort_by_cached_key(|p| p.name.to_lowercase());
    let name = match &action {
        Action::Profile(name) => Some(name),
        _ => None,
    };
    if profiles.is_empty() {
        terminal.line("no ssh connections yet")?;
        terminal.line(
            "add them in Shellfox › Settings › SSH connections  (or import from PuTTY there)",
        )?;
        return Ok(if name.is_some() {
            1
        } else if terminal.tty {
            0
        } else {
            2
        });
    }
    let selected = if let Some(name) = name {
        let name_key = name.to_lowercase();
        let Some(selected) = profiles.iter().find(|p| p.name.to_lowercase() == name_key) else {
            let shown = escape(&serde_json::to_string(name)?);
            terminal.line(&terminal.style("31", &format!("no connection named {shown}")))?;
            for p in profiles {
                terminal.line(&terminal.dim(&escape(&p.name)))?;
            }
            return Ok(1);
        };
        selected
    } else if !terminal.tty {
        for p in profiles {
            terminal.line(&escape(&p.name))?;
        }
        terminal.help(false)?;
        return Ok(2);
    } else {
        // Electron keeps one raw-mode guard across selection and connection.
        return select_and_connect(terminal, store, &profiles).await;
    };
    let raw = terminal.start()?;
    connect_selected(terminal, store, selected, raw).await
}
async fn select_and_connect(
    terminal: &Terminal,
    store: Store,
    profiles: &[ProfileDto],
) -> Result<i32> {
    let raw = terminal.start()?;
    let selected = terminal.pick(profiles).await?;
    connect_selected(terminal, store, &profiles[selected], raw).await
}
async fn connect_selected(
    terminal: &Terminal,
    store: Store,
    selected: &ProfileDto,
    raw: Option<terminal::Raw<'_>>,
) -> Result<i32> {
    let (profile, directory) = store.selected(selected).await?;
    let code = session::connect(profile, directory, terminal).await?;
    drop(raw);
    terminal.write("\n")?;
    let status = if code == 0 {
        String::new()
    } else {
        format!(" (exit {code})")
    };
    terminal.line(&terminal.dim(&format!(
        "connection to {} closed{status}",
        escape(&selected.name)
    )))?;
    Ok(code)
}

struct Store {
    data: PathBuf,
    helper: Option<(PathBuf, [u8; 32])>,
    live: Option<LocalClient>,
    offline: Option<Arc<PrivateDirectory>>,
}
impl Store {
    async fn open(opts: Options) -> Result<Self> {
        let data = opts
            .data
            .or_else(|| std::env::var_os("SHELLFOX_USER_DATA").map(PathBuf::from))
            .map(Ok)
            .unwrap_or_else(app_paths::default_data_root)?;
        let data = std::path::absolute(data)?;
        let helper = match (opts.helper, opts.digest) {
            (Some(path), Some(digest)) => Some((
                path,
                crate::args::parse_digest(digest.to_str().context("Invalid SHA-256 encoding.")?)?,
            )),
            (None, None) if app_paths::is_packaged() => Some(app_paths::packaged_helper()?),
            (None, None) => None,
            _ => bail!("Supply both --helper and --helper-sha256."),
        };
        let (live, offline) = if local::ssh_endpoint_exists(&data)? {
            let (path, digest) = helper.as_ref().context(
                "A trusted helper is required to authenticate the running Shellfox host.",
            )?;
            (
                Some(LocalClient::connect(&data, path, *digest).await?),
                None,
            )
        } else {
            (None, local::open_ssh_directory(&data, helper.as_ref())?)
        };
        Ok(Self {
            data,
            helper,
            live,
            offline,
        })
    }
    async fn read(&self, request: SshRead) -> Result<SshReadReply> {
        match &self.live {
            Some(client) => client.request(&request).await,
            None => local::read_ssh_profiles(self.offline.as_ref(), request),
        }
    }
    async fn list(&self) -> Result<Vec<ProfileDto>> {
        match self.read(SshRead::List {}).await? {
            SshReadReply::Profiles(profiles) => Ok(profiles),
            _ => bail!("Invalid SSH profile list."),
        }
    }
    async fn selected(
        self,
        selected: &ProfileDto,
    ) -> Result<(shellfox_ssh::StoredProfile, Arc<PrivateDirectory>)> {
        let SshReadReply::Connection(Some(profile)) =
            self.read(SshRead::Connection { id: selected.id }).await?
        else {
            bail!("SSH profile is no longer available.");
        };
        ensure!(profile.id == selected.id, "Invalid SSH profile reply.");
        let directory = match self.offline {
            Some(directory) => directory,
            None => local::open_ssh_directory(&self.data, self.helper.as_ref())?
                .context("SSH private directory is unavailable.")?,
        };
        Ok((*profile, directory))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn electron_profile_forms_are_unambiguous() {
        let parse = |args: &[&str]| action(&args.iter().map(OsString::from).collect::<Vec<_>>());
        assert_eq!(parse(&[]), Some(Action::Choose));
        for words in [
            &["fixture"][..],
            &["-p", "fixture"],
            &["--profile", "fixture"],
        ] {
            assert_eq!(parse(words), Some(Action::Profile("fixture".into())));
        }
        for words in [
            &["-p"][..],
            &["--profile", "-h"],
            &["fixture", "-p", "other"],
            &["--profile=fixture"],
        ] {
            assert_eq!(parse(words), None);
        }
    }
}
