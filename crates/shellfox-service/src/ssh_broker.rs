//! Private OS IPC only. No password or key passphrase is ever an environment value.
use super::{work::Work, *};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::mpsc,
    thread::JoinHandle,
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::Zeroizing;
const ENDPOINT: &str = "SHELLFOX_SSH_ASKPASS_ENDPOINT";
const DIRECTORY: &str = "SHELLFOX_SSH_ASKPASS_DIRECTORY";
const TOKEN: &str = "SHELLFOX_SSH_ASKPASS_TOKEN";
const MAX_FRAME: usize = 512 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    token: Secret,
    prompt: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    answer: Option<Secret>,
}

pub(super) struct Broker {
    cancel: CancellationToken,
    thread: Option<JoinHandle<()>>,
    pub environment: BTreeMap<String, String>,
}
impl Broker {
    pub fn start(
        directory: &Path,
        program: &Path,
        work: Work,
        mut password: Option<Secret>,
        identity: Option<(PathBuf, Secret)>,
    ) -> Result<Self> {
        anyhow::ensure!(
            program.is_absolute() && program.is_file(),
            "A trusted absolute askpass executable is required."
        );
        let cancel = work.cancel.clone();
        let stop = cancel.clone();
        let directory = directory.to_owned();
        let ready_directory = directory.clone();
        let token = Secret::new(format!(
            "{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        ))?;
        let ready_token = token.clone();
        let (ready, rx) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("shellfox-ssh-askpass".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build();
                let Ok(runtime) = runtime else {
                    let _ = ready.send(Err(()));
                    return;
                };
                runtime.block_on(async move {
                    let Ok(mut listener) = crate::platform::Listener::bind(&directory) else {
                        let _ = ready.send(Err(()));
                        return;
                    };
                    if ready.send(Ok(listener.address())).is_err() {
                        return;
                    }
                    for _ in 0..64 {
                        let accepted = tokio::select! {
                            _=stop.cancelled()=>break,
                            _=work.lost.cancelled()=>break,
                            _=work.cancel.cancelled()=>break,
                            value=listener.accept()=>value,
                        };
                        let Ok(mut stream) = accepted else { break };
                        let serve = async {
                            let query: Query = read(&mut stream).await?;
                            stream.verify_peer()?;
                            anyhow::ensure!(
                                bool::from(
                                    query
                                        .token
                                        .expose()
                                        .as_bytes()
                                        .ct_eq(token.expose().as_bytes())
                                ),
                                "Invalid askpass capability."
                            );
                            work.check()?;
                            let response = if let Some((path, secret)) = &identity {
                                // Keyboard-interactive is disabled for prepared identities.
                                // This exact prompt is produced by the trusted local ssh client.
                                let expected = format!(
                                    "Enter passphrase for key '{}': ",
                                    path.to_string_lossy()
                                );
                                if query.prompt == expected
                                    || query.prompt == expected.replace('\\', "/")
                                {
                                    Some(secret.clone())
                                } else {
                                    password_for_prompt(&work, &query.prompt, &mut password)?
                                }
                            } else {
                                password_for_prompt(&work, &query.prompt, &mut password)?
                            };
                            work.check()?;
                            write(&mut stream, &Answer { answer: response }).await
                        };
                        tokio::select! {
                            _=stop.cancelled()=>break,
                            _=work.lost.cancelled()=>break,
                            _=work.cancel.cancelled()=>break,
                            _=tokio::time::timeout(Duration::from_secs(125),serve)=>{},
                        }
                    }
                });
            })?;
        let address = match rx.recv_timeout(Duration::from_secs(8)) {
            Ok(Ok(address)) => address,
            _ => {
                cancel.cancel();
                let _ = thread.join();
                anyhow::bail!("Private SSH askpass broker failed to start.")
            }
        };
        let environment = BTreeMap::from([
            ("SSH_ASKPASS".into(), program.to_string_lossy().into_owned()),
            ("SSH_ASKPASS_REQUIRE".into(), "force".into()),
            ("DISPLAY".into(), "shellfox-askpass:0".into()),
            (ENDPOINT.into(), address),
            (
                DIRECTORY.into(),
                ready_directory.to_string_lossy().into_owned(),
            ),
            // Random IPC capability only, never an account password/key passphrase.
            (TOKEN.into(), ready_token.expose().to_owned()),
        ]);
        Ok(Self {
            cancel,
            thread: Some(thread),
            environment,
        })
    }
    pub fn stop(&mut self) -> Result<()> {
        self.cancel.cancel();
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("Askpass broker shutdown failed."))?;
        }
        Ok(())
    }
}
impl Drop for Broker {
    fn drop(&mut self) {
        self.cancel.cancel();
        let _ = self.stop();
        for value in self.environment.values_mut() {
            zeroize::Zeroize::zeroize(value);
        }
    }
}
fn password_for_prompt(
    work: &Work,
    prompt: &str,
    password: &mut Option<Secret>,
) -> Result<Option<Secret>> {
    // Password prompts are local OpenSSH strings. Unknown/KI challenges remain
    // explicit UI prompts; never send a stored secret to an arbitrary challenge.
    if prompt.ends_with("'s password: ") && !prompt.contains(['\r', '\n']) && password.is_some() {
        return Ok(password.take());
    }
    Ok(Some(work.prompt(prompt, false)?))
}
async fn read<T: serde::de::DeserializeOwned>(
    stream: &mut crate::platform::LocalStream,
) -> Result<T> {
    let length = stream.read_u32().await? as usize;
    anyhow::ensure!(
        length > 0 && length <= MAX_FRAME,
        "Private SSH frame exceeds its bound."
    );
    let mut bytes = Zeroizing::new(vec![0; length]);
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("Invalid private SSH frame."))
}
async fn write<T: Serialize>(stream: &mut crate::platform::LocalStream, value: &T) -> Result<()> {
    let bytes = Zeroizing::new(
        serde_json::to_vec(value).map_err(|_| anyhow::anyhow!("Invalid private SSH reply."))?,
    );
    anyhow::ensure!(
        bytes.len() <= MAX_FRAME,
        "Private SSH reply exceeds its bound."
    );
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}
/// Hidden askpass entry point, before creating a UI. Call only when the host's
/// trusted entry dispatcher selected askpass mode, never its supervisor mode.
/// Print the returned secret ONLY to the SSH-owned captured stdout, then exit.
pub async fn askpass_from_environment(prompt: &str) -> Result<Secret> {
    anyhow::ensure!(prompt.len() <= 8192, "SSH prompt exceeds its bound.");
    let endpoint =
        std::env::var(ENDPOINT).map_err(|_| anyhow::anyhow!("No private SSH broker."))?;
    let directory =
        std::env::var(DIRECTORY).map_err(|_| anyhow::anyhow!("No private SSH broker."))?;
    let token =
        Secret::new(std::env::var(TOKEN).map_err(|_| anyhow::anyhow!("No private SSH broker."))?)?;
    anyhow::ensure!(
        std::env::var("SSH_ASKPASS_REQUIRE").as_deref() == Ok("force")
            && Path::new(&directory).is_absolute()
            && directory.len() <= 4096
            && !directory.chars().any(char::is_control)
            && endpoint.len() <= 4096
            && !endpoint.chars().any(char::is_control)
            && token.expose().len() == 64
            && token.expose().bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid private SSH callback descriptor."
    );
    request(&endpoint, Path::new(&directory), token, prompt).await
}
/// Call after native supervisor/CLI modes have been discriminated, before any UI.
/// Returns None for normal invocations. Only the SSH-owned stdout receives the answer.
/// A dispatch/fail-closed hint only, never authentication or executable approval.
pub fn has_askpass_environment() -> bool {
    [ENDPOINT, DIRECTORY, TOKEN]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
}
pub fn askpass_entry(arguments: &[std::ffi::OsString]) -> Option<i32> {
    if !has_askpass_environment() {
        return None;
    }
    // An incomplete callback must fail closed, never fall through to normal CLI/GUI.
    if arguments.len() != 1 {
        return Some(1);
    }
    let Some(prompt) = arguments[0].to_str() else {
        return Some(1);
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
    else {
        return Some(1);
    };
    let Ok(answer) = runtime.block_on(askpass_from_environment(prompt)) else {
        return Some(1);
    };
    if answer.expose().contains('\0') {
        return Some(1);
    }
    use std::io::Write;
    let result = std::io::stdout()
        .write_all(answer.expose().as_bytes())
        .and_then(|_| std::io::stdout().write_all(b"\n"));
    Some(if result.is_ok() { 0 } else { 1 })
}
pub(super) async fn request(
    endpoint: &str,
    directory: &Path,
    token: Secret,
    prompt: &str,
) -> Result<Secret> {
    let future = async {
        let mut stream = crate::platform::connect(endpoint, directory).await?;
        write(
            &mut stream,
            &Query {
                token,
                prompt: prompt.into(),
            },
        )
        .await?;
        let answer: Answer = read(&mut stream).await?;
        stream.verify_peer()?;
        answer
            .answer
            .ok_or_else(|| anyhow::anyhow!("SSH prompt cancelled."))
    };
    tokio::time::timeout(Duration::from_secs(125), future)
        .await
        .map_err(|_| anyhow::anyhow!("SSH prompt expired."))?
}
