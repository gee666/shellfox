use super::{
    auth::{self, Agent},
    io::{exchange, output},
    terminal::Terminal,
};
use anyhow::{bail, Result};
use russh::{client, Channel, ChannelMsg, ChannelOpenFailure, Disconnect, Pty};
use shellfox_ssh::{
    pty, Error, KnownHosts, PrivateDirectory, StoredProfile, TrustDecision, TrustStatus,
};
use std::{
    io,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    net::{lookup_host, TcpStream},
    sync::Semaphore,
};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(super) struct ConnectionError(pub(super) String);

pub(super) struct Handler {
    terminal: Terminal,
    directory: Arc<PrivateDirectory>,
    host: String,
    port: u16,
    forward: Option<Agent>,
    forwarding_enabled: Arc<AtomicBool>,
    forwarded_channels: Arc<Semaphore>,
}
impl client::Handler for Handler {
    type Error = anyhow::Error;
    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool> {
        if server_public_key.certificate().is_some() {
            bail!("unsupported SSH host certificate");
        }
        let key = server_public_key.public_key().to_bytes()?;
        let known = KnownHosts::new(self.directory.clone());
        let status = known
            .inspect(&self.host, self.port, &key)
            .map_err(|error| self.trust_error(error))?;
        if let TrustStatus::Unknown(info) = status {
            self.terminal.line(
                &self
                    .terminal
                    .dim(&format!("{} {}", info.key_type, info.fingerprint)),
            )?;
            let answer = self
                .terminal
                .prompt("Trust this host? [y/N] ", false)
                .await?;
            if !answer.trim().eq_ignore_ascii_case("y")
                && !answer.trim().eq_ignore_ascii_case("yes")
            {
                bail!("host key was not trusted");
            }
            known
                .verify(&self.host, self.port, &key, |_, _, _| {
                    Ok(TrustDecision::TrustAndSave)
                })
                .map_err(|error| self.trust_error(error))?;
        }
        Ok(true)
    }
    async fn server_channel_open_agent_forward(
        &mut self,
        channel: Channel<client::Msg>,
        reply: client::ChannelOpenHandle,
        _: &mut client::Session,
    ) -> Result<()> {
        if self.forwarding_enabled.load(Ordering::Acquire) {
            if let Some(agent) = &self.forward {
                if let Ok(permit) = self.forwarded_channels.clone().try_acquire_owned() {
                    if let Ok(local) = agent.connect().await {
                        reply.accept().await;
                        tokio::spawn(async move {
                            let _permit = permit;
                            let _ = tokio::io::copy_bidirectional(
                                &mut local.into_inner(),
                                &mut channel.into_stream(),
                            )
                            .await;
                        });
                        return Ok(());
                    }
                }
            }
        }
        reply
            .reject(ChannelOpenFailure::AdministrativelyProhibited)
            .await;
        Ok(())
    }
}
impl Handler {
    fn trust_error(&self, error: Error) -> anyhow::Error {
        match error {
            Error::HostKeyChanged => anyhow::anyhow!("host key changed for {}:{}; refusing connection. Remove this host from {} only after verifying the new key.", self.host, self.port, self.directory.path().join("ssh-known-hosts.json").display()),
            Error::Corrupt => anyhow::anyhow!("ssh-known-hosts.json is corrupt; repair it before connecting."),
            Error::Busy => anyhow::anyhow!("known hosts are busy; retry or remove the stale .lock file."),
            error => error.into(),
        }
    }
}

pub(super) async fn connect(
    mut profile: StoredProfile,
    directory: Arc<PrivateDirectory>,
    terminal: &Terminal,
) -> Result<i32> {
    let options = profile.connection_options.clone().unwrap_or_default();
    if let Some(proxy) = &options.unsupported_proxy {
        let address = proxy
            .host
            .as_ref()
            .filter(|h| !h.is_empty())
            .map(|host| {
                format!(
                    " {host}{}",
                    proxy
                        .port
                        .filter(|p| *p != 0)
                        .map_or(String::new(), |p| format!(":{p}"))
                )
            })
            .unwrap_or_default();
        return Err(ConnectionError(format!("PuTTY profile requires {} proxy/jump host{address}; proxies and jump hosts are not supported. Disable the proxy in PuTTY and re-import, or add a direct manual connection.", proxy.method)).into());
    }
    if profile.user.is_empty() {
        profile.user = terminal.prompt("login as: ", false).await?.to_string();
        if profile.user.is_empty() {
            bail!("login name is required");
        }
    }
    let key = match &profile.key_file {
        Some(file) => Some(auth::load_key(&directory, file, terminal).await?),
        None => None,
    };
    let agents = Agent::endpoints();
    let mut forward = None;
    if options.agent_forward == Some(true) {
        forward = Agent::available(&agents).await;
        if forward.is_none() {
            terminal
                .line(&terminal.dim("no SSH agent available; agent forwarding is unavailable"))?;
        }
    }
    terminal.line(&terminal.dim(&format!(
        "connecting to {} ...",
        super::picker::target(&profile.user, &profile.host, profile.port)
    )))?;
    let forwarding_enabled = Arc::new(AtomicBool::new(false));
    let handler = Handler {
        terminal: terminal.clone(),
        directory,
        host: profile.host.clone(),
        port: profile.port,
        forward: forward.clone(),
        forwarding_enabled: forwarding_enabled.clone(),
        forwarded_channels: Arc::new(Semaphore::new(16)),
    };
    let config = Arc::new(client::Config {
        inactivity_timeout: None,
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 0,
        ..Default::default()
    });
    let opening = tokio::time::timeout(Duration::from_secs(120), async {
        let socket = connect_tcp(&profile, options.address_family).await?;
        let mut handle = client::connect_stream(config, socket, handler)
            .await
            .map_err(before_login)?;
        let enabled_agents = if options.try_agent == Some(false) {
            &[][..]
        } else {
            agents.as_slice()
        };
        auth::authenticate(&mut handle, &profile, key, enabled_agents, terminal)
            .await
            .map_err(before_login)?;
        Ok::<_, anyhow::Error>(handle)
    });
    let handle = tokio::select! {
        result = opening => result.map_err(|_| ConnectionError("could not connect: Timed out while waiting for handshake".into()))??,
        cancelled = terminal.cancel_before_login(), if terminal.tty => { cancelled?; unreachable!("cancel waiter returns an error") }
    };
    let mut channel = handle.channel_open_session().await?;
    if forward.is_some() {
        forwarding_enabled.store(true, Ordering::Release);
        channel.agent_forward(false).await?;
    }
    request_shell(&mut channel, profile.remote_cwd.as_deref(), terminal).await?;
    let result = exchange(channel, terminal).await;
    let _ = handle.disconnect(Disconnect::ByApplication, "", "").await;
    result
}
fn before_login(error: anyhow::Error) -> anyhow::Error {
    match error.downcast_ref::<russh::Error>() {
        Some(russh::Error::Disconnect | russh::Error::SendError) => {
            ConnectionError("connection closed before login".into()).into()
        }
        Some(_) => ConnectionError(format!("could not connect: {error}")).into(),
        None => error,
    }
}
async fn connect_tcp(profile: &StoredProfile, family: Option<u8>) -> Result<TcpStream> {
    let addrs = lookup_host((profile.host.as_str(), profile.port))
        .await
        .map_err(|error| {
            let reason = match error.kind() {
                io::ErrorKind::PermissionDenied => error.to_string(),
                _ => format!("getaddrinfo ENOTFOUND {}", profile.host),
            };
            ConnectionError(format!("could not connect: {reason}"))
        })?;
    let mut last = None;
    for addr in addrs.filter(|addr| match family {
        Some(4) => addr.is_ipv4(),
        Some(6) => addr.is_ipv6(),
        _ => true,
    }) {
        match TcpStream::connect(addr).await {
            Ok(socket) => {
                let keepalive = socket2::TcpKeepalive::new().with_time(Duration::from_secs(30));
                let _ = socket2::SockRef::from(&socket).set_tcp_keepalive(&keepalive);
                return Ok(socket);
            }
            Err(error) => last = Some((error, addr)),
        }
    }
    let reason = last.map_or_else(
        || format!("getaddrinfo ENOTFOUND {}", profile.host),
        |(e, a)| socket_error(e, a),
    );
    Err(ConnectionError(format!("could not connect: {reason}")).into())
}
fn socket_error(error: io::Error, addr: SocketAddr) -> String {
    let code = match error.kind() {
        io::ErrorKind::ConnectionRefused => "ECONNREFUSED",
        io::ErrorKind::ConnectionReset => "ECONNRESET",
        io::ErrorKind::TimedOut => "ETIMEDOUT",
        io::ErrorKind::HostUnreachable => "EHOSTUNREACH",
        io::ErrorKind::NetworkUnreachable => "ENETUNREACH",
        _ => return error.to_string(),
    };
    format!("connect {code} {}:{}", addr.ip(), addr.port())
}

async fn request_shell(
    channel: &mut Channel<client::Msg>,
    cwd: Option<&str>,
    terminal: &Terminal,
) -> Result<()> {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let modes: Vec<_> = pty::TTY_MODES
        .iter()
        .filter_map(|&(code, value)| Pty::from_u8(code).map(|mode| (mode, value)))
        .collect();
    for (name, value) in pty::remote_environment(
        std::env::vars()
            .collect::<Vec<_>>()
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str())),
    ) {
        channel.set_env(false, name, value).await?;
    }
    channel
        .request_pty(
            true,
            "xterm-256color",
            cols.into(),
            rows.into(),
            0,
            0,
            &modes,
        )
        .await?;
    wait_success(channel, "unable to request a pseudo-terminal").await?;
    if let Some(cwd) = cwd.filter(|s| !s.is_empty()) {
        channel
            .exec(true, remote_command(cwd, terminal.color))
            .await?;
    } else {
        channel.request_shell(true).await?;
    }
    wait_success(channel, "unable to start shell").await
}
async fn wait_success(channel: &mut Channel<client::Msg>, failure: &str) -> Result<()> {
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Success) => return Ok(()),
            Some(ChannelMsg::Failure) => bail!("{failure}"),
            Some(ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. }) => {
                output(&data)?
            }
            Some(ChannelMsg::Close) | None => {
                return Err(ConnectionError("connection lost".into()).into())
            }
            _ => {}
        }
    }
}
fn remote_command(cwd: &str, color: bool) -> String {
    let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let directory = if cwd == "~" {
        "\"$HOME\"".into()
    } else if let Some(path) = cwd.strip_prefix("~/") {
        format!("\"$HOME\"/{}", quote(path))
    } else {
        quote(cwd)
    };
    let warning = if color {
        "\\033[2m  remote folder unavailable; staying in home\\033[0m"
    } else {
        "  remote folder unavailable; staying in home"
    };
    format!("cd -- {directory} 2>/dev/null || printf '{warning}\\n' >&2; exec \"${{SHELL:-/bin/sh}}\" -il")
}
