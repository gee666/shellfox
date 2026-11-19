use super::{session::Handler, terminal::Terminal};
use anyhow::{bail, Result};
use russh::{
    client::{Handle, KeyboardInteractiveAuthResponse},
    keys::{
        self,
        agent::client::{AgentClient, AgentStream},
        PrivateKey, PrivateKeyWithHashAlg,
    },
    MethodKind, MethodSet,
};
use shellfox_ssh::{keys::parse_private_key, Error, PrivateDirectory, StoredProfile};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub(super) async fn load_key(
    directory: &PrivateDirectory,
    file: &str,
    terminal: &Terminal,
) -> Result<PrivateKey> {
    let path = key_path(file).await?;
    let bytes = directory.read_key_file(&path)?;
    let key = match parse_private_key(&bytes, None) {
        Ok(key) => key,
        Err(Error::PassphraseRequired) => {
            let mut attempt = 1;
            loop {
                let passphrase = terminal.secret("Passphrase: ").await?;
                match parse_private_key(&bytes, Some(&passphrase)) {
                    Ok(key) => break key,
                    Err(error) if attempt == 3 => return Err(error.into()),
                    Err(_) => terminal.line(&terminal.dim("incorrect passphrase"))?,
                }
                attempt += 1;
            }
        }
        Err(error) => return Err(error.into()),
    };
    Ok(keys::decode_secret_key(&key.to_openssh_in_memory()?, None)?)
}
async fn key_path(path: &str) -> Result<PathBuf> {
    #[cfg(unix)]
    if std::env::var_os("WSL_DISTRO_NAME").is_some() {
        let windows_path = path.starts_with("\\\\")
            || matches!(path.as_bytes(), [letter, b':', b'\\' | b'/', ..] if letter.is_ascii_alphabetic());
        if windows_path {
            let mut command = tokio::process::Command::new("wslpath");
            command.args(["-u", path]).kill_on_drop(true);
            let output = tokio::time::timeout(Duration::from_secs(5), command.output()).await??;
            anyhow::ensure!(
                output.status.success(),
                "could not resolve private key path in WSL"
            );
            return Ok(PathBuf::from(std::str::from_utf8(&output.stdout)?.trim()));
        }
    }
    Ok(Path::new(path).to_owned())
}

#[derive(Clone)]
pub(super) enum Agent {
    #[cfg(unix)]
    Unix(PathBuf),
    #[cfg(windows)]
    Pipe(PathBuf),
    #[cfg(windows)]
    Pageant,
}
type AgentConnection = AgentClient<Box<dyn AgentStream + Send + Unpin>>;
impl Agent {
    pub fn endpoints() -> Vec<Self> {
        #[cfg(unix)]
        return std::env::var_os("SSH_AUTH_SOCK")
            .filter(|s| !s.is_empty())
            .map(|path| Self::Unix(path.into()))
            .into_iter()
            .collect();
        #[cfg(windows)]
        return vec![
            Self::Pageant,
            Self::Pipe(PathBuf::from(r"\\.\pipe\openssh-ssh-agent")),
        ];
        #[cfg(not(any(unix, windows)))]
        Vec::new()
    }
    pub async fn connect(&self) -> Result<AgentConnection> {
        let connect = async {
            Ok(match self {
                #[cfg(unix)]
                Self::Unix(path) => AgentClient::connect_uds(path).await?.dynamic(),
                #[cfg(windows)]
                Self::Pipe(path) => AgentClient::connect_named_pipe(path).await?.dynamic(),
                #[cfg(windows)]
                Self::Pageant => AgentClient::connect_pageant().await?.dynamic(),
            })
        };
        tokio::time::timeout(Duration::from_secs(1), connect).await?
    }
    pub async fn available(agents: &[Self]) -> Option<Self> {
        let mut empty = None;
        for agent in agents {
            let identities = tokio::time::timeout(Duration::from_millis(1500), async {
                Ok::<_, anyhow::Error>(agent.connect().await?.request_identities().await?)
            })
            .await;
            if let Ok(Ok(keys)) = identities {
                if !keys.is_empty() {
                    return Some(agent.clone());
                }
                empty = Some(agent.clone());
            }
        }
        empty
    }
}

pub(super) async fn authenticate(
    handle: &mut Handle<Handler>,
    profile: &StoredProfile,
    key: Option<PrivateKey>,
    agents: &[Agent],
    terminal: &Terminal,
) -> Result<()> {
    use russh::client::AuthResult;
    use AuthResult::{Failure, Success};
    let user = &profile.user;
    let mut remaining: MethodSet = match handle.authenticate_none(user).await? {
        Success => return Ok(()),
        Failure {
            remaining_methods, ..
        } => remaining_methods,
    };
    macro_rules! attempt {
        ($expr:expr) => {
            match $expr? {
                Success => return Ok(()),
                Failure {
                    remaining_methods, ..
                } => remaining = remaining_methods,
            }
        };
    }
    if remaining.contains(&MethodKind::PublicKey) {
        if let Some(key) = key {
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            attempt!(
                handle
                    .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                    .await
            );
        }
    }
    if remaining.contains(&MethodKind::PublicKey) {
        for endpoint in agents {
            let Ok(mut agent) = endpoint.connect().await else {
                continue;
            };
            if let Some(binding) = handle.agent_session_binding() {
                // Older agents do not implement OpenSSH's binding extension.
                let _ = agent.bind_session(binding, false).await;
            }
            let Ok(identities) = agent.request_identities().await else {
                continue;
            };
            for identity in identities {
                if !remaining.contains(&MethodKind::PublicKey) {
                    break;
                }
                let hash = handle.best_supported_rsa_hash().await?.flatten();
                match handle
                    .authenticate_publickey_with(
                        user,
                        identity.public_key().into_owned(),
                        hash,
                        &mut agent,
                    )
                    .await
                {
                    Ok(Success) => return Ok(()),
                    Ok(Failure {
                        remaining_methods, ..
                    }) => remaining = remaining_methods,
                    Err(_) => {}
                }
            }
        }
    }
    let mut password_tries = usize::from(profile.password.is_some());
    if remaining.contains(&MethodKind::Password) {
        if let Some(password) = &profile.password {
            attempt!(handle.authenticate_password(user, password.expose()).await);
        }
    }
    if remaining.contains(&MethodKind::KeyboardInteractive) {
        let mut response = handle
            .authenticate_keyboard_interactive_start(user, None::<String>)
            .await?;
        loop {
            match response {
                KeyboardInteractiveAuthResponse::Success => return Ok(()),
                KeyboardInteractiveAuthResponse::Failure {
                    remaining_methods, ..
                } => {
                    remaining = remaining_methods;
                    break;
                }
                KeyboardInteractiveAuthResponse::InfoRequest {
                    instructions,
                    prompts,
                    ..
                } => {
                    if !instructions.is_empty() {
                        terminal.line(&terminal.dim(&challenge(&instructions)))?;
                    }
                    let mut answers = Vec::with_capacity(prompts.len());
                    for prompt in prompts {
                        let mut answer = terminal
                            .prompt(&challenge(&prompt.prompt), !prompt.echo)
                            .await?;
                        answers.push(std::mem::take(&mut *answer));
                    }
                    response = handle
                        .authenticate_keyboard_interactive_respond(answers)
                        .await?;
                }
            }
        }
    }
    while password_tries < 3 && remaining.contains(&MethodKind::Password) {
        password_tries += 1;
        let label = format!(
            "{}@{}'s password: ",
            super::picker::escape(user),
            super::picker::escape(&profile.host)
        );
        let answer = terminal.secret(&label).await?;
        attempt!(handle.authenticate_password(user, answer.expose()).await);
    }
    bail!("authentication failed")
}
fn challenge(text: &str) -> String {
    let sanitized = text.replace(|c: char| c <= '\u{1f}' || c == '\u{7f}', " ");
    super::picker::escape(&sanitized)
}
