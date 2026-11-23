use anyhow::{bail, ensure, Context, Result};
use shellfox_core::WslLaunchAccount;
use shellfox_files::{
    wsl_files_ipc::{self as wire, Reply, Request},
    TransferReply, TransferRequest, CHUNK_BYTES,
};
use shellfox_terminal::wsl::OsWsl;
use std::{
    io::{BufReader, Write},
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::Duration,
};

type ReplyPacket = (Reply, Vec<u8>);
const WORKER: Option<&[u8]> = include!(concat!(env!("OUT_DIR"), "/wsl-files-asset.rs"));
const BOOTSTRAP: &str = include_str!("wsl_files_bootstrap.py");

pub struct Worker {
    child: Option<Child>,
    requests: Option<SyncSender<(Request, Vec<u8>)>>,
    replies: Receiver<Result<ReplyPacket>>,
}

impl Worker {
    pub fn start(distro: &str, account: &WslLaunchAccount) -> Result<Self> {
        let executable = WORKER.context("This Windows build lacks the WSL Files helper. Install a current Shellfox build for your Windows architecture.")?;
        let machine = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            _ => bail!("WSL Files requires x64 or ARM64 Windows."),
        };
        let wsl = OsWsl::discover()?;
        let mut child = Command::new(wsl.path())
            .args([
                "--distribution",
                distro,
                "--user",
                account.name(),
                "--cd",
                "/",
                "--exec",
                "/usr/bin/python3",
                "-I",
                "-S",
                "-c",
                BOOTSTRAP,
            ])
            .args([
                distro,
                machine,
                &account.uid().to_string(),
                &executable.len().to_string(),
            ])
            .env_remove("WSLENV")
            .env_remove("SHELLFOX_TERMINAL_MARKER")
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Could not start WSL Files in the tab's distribution.")?;
        let input = child.stdin.take();
        let output = child.stdout.take();
        let (requests, outgoing) = mpsc::sync_channel::<(Request, Vec<u8>)>(1);
        let (incoming, replies) = mpsc::sync_channel(1);
        let worker = Self {
            child: Some(child),
            requests: Some(requests),
            replies,
        };
        let mut input = input.context("Missing WSL worker stdin")?;
        let mut output = BufReader::new(output.context("Missing WSL worker stdout")?);
        let _ = thread::Builder::new()
            .name("wsl-files-write".into())
            .spawn(move || {
                if input.write_all(executable).is_ok() {
                    for (request, data) in outgoing {
                        if wire::write(&mut input, &request, &data).is_err() {
                            break;
                        }
                    }
                }
            })?;
        let _ = thread::Builder::new()
            .name("wsl-files-read".into())
            .spawn(move || loop {
                match wire::read(&mut output) {
                    Ok(Some(packet)) => {
                        if incoming.send(Ok(packet)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = incoming.send(Err(error.into()));
                        break;
                    }
                }
            })?;
        let (reply, bytes) = worker.receive(Duration::from_secs(15)).context(
            "WSL Files could not start under the saved launch account. Check that the account still exists, /usr/bin/python3 is available in WSL 2, and Shellfox matches the guest architecture.",
        )?;
        let Reply::Ready {
            version: wire::VERSION,
            uid,
        } = reply
        else {
            bail!("Unexpected WSL Files worker version. Reinstall Shellfox.");
        };
        ensure!(bytes.is_empty(), "Invalid WSL Files worker identity reply.");
        account.verify_uid(uid)?;
        Ok(worker)
    }

    pub fn exchange(&self, request: Request, bytes: Vec<u8>) -> Result<ReplyPacket> {
        self.requests
            .as_ref()
            .context("WSL Files closed")?
            .try_send((request, bytes))
            .map_err(|_| anyhow::anyhow!("Could not send request to WSL Files"))?;
        self.receive(Duration::from_secs(30))
    }

    pub(crate) fn transfer(&self, mut request: TransferRequest) -> Result<TransferReply> {
        let bytes = match &mut request {
            TransferRequest::Write(input) => std::mem::take(&mut input.data),
            _ => Vec::new(),
        };
        let (reply, bytes) = self.exchange(Request::Transfer(request), bytes)?;
        let mut reply = match reply {
            Reply::Transfer(reply) => reply,
            Reply::Error(error) if bytes.is_empty() => return Err(error.into()),
            _ => bail!("Invalid WSL transfer response"),
        };
        match &mut reply {
            TransferReply::Chunk(chunk) if chunk.data.is_empty() && bytes.len() <= CHUNK_BYTES => {
                chunk.data = bytes
            }
            TransferReply::Chunk(_) => bail!("Invalid WSL transfer chunk"),
            _ if !bytes.is_empty() => bail!("Unexpected WSL transfer payload"),
            _ => {}
        }
        Ok(reply)
    }

    fn receive(&self, limit: Duration) -> Result<ReplyPacket> {
        self.replies
            .recv_timeout(limit)
            .context("WSL Files did not return a complete result")?
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing stdin lets the guest release its held descriptors and clean its journal.
        self.requests.take();
        if let Some(mut child) = self.child.take() {
            thread::spawn(move || {
                use std::os::windows::io::AsRawHandle;
                use windows_sys::Win32::{
                    Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject,
                };
                // SAFETY: `child` owns the process handle for the whole wait.
                let exited = unsafe { WaitForSingleObject(child.as_raw_handle().cast(), 5_000) }
                    == WAIT_OBJECT_0;
                if !exited {
                    let _ = child.kill();
                }
                let _ = child.wait();
            });
        }
    }
}
