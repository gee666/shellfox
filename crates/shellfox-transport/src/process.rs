use crate::NetworkStatus;
use crate::{
    config::VerifiedHelper,
    protocol::{self, Frame},
    HelperError,
};
use serde_json::{json, Value};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
pub(crate) struct Process {
    pub(crate) child: Child,
    _artifact: VerifiedHelper,
    stdin: Option<ChildStdin>,
    pub(crate) frames: mpsc::Receiver<Result<Vec<u8>, HelperError>>,
    pub(crate) statuses: watch::Receiver<Option<NetworkStatus>>,
    tasks: Vec<JoinHandle<()>>,
    next_id: u64,
    failed: bool,
    exit_confirmed: bool,
    kill_requested: bool,
    stop_attempted: bool,
    stop_acknowledged: bool,
    pub(crate) stopped: CancellationToken,
}
impl Process {
    pub(crate) fn spawn(executable: &VerifiedHelper, probe: bool) -> Result<Self, HelperError> {
        executable.verify()?;
        let mut command = Command::new(executable.path());
        if probe {
            command.arg("--probe");
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // The helper clears TS_* too. Do not pass ambient enrollment credentials to it.
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy().to_ascii_uppercase();
            if name.starts_with("TS_") || name.starts_with("TSNET_") {
                command.env_remove(key);
            }
        }
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let mut child = command.spawn().map_err(|_| HelperError::Spawn)?;
        let stdin = child.stdin.take().ok_or(HelperError::Spawn)?;
        let stdout = child.stdout.take().ok_or(HelperError::Spawn)?;
        let mut stderr = child.stderr.take().ok_or(HelperError::Spawn)?;
        let (send, frames) = mpsc::channel(2);
        let (status_send, statuses) = watch::channel(None);
        let stopped = CancellationToken::new();
        let failure = stopped.clone();
        let reader = tokio::spawn(async move {
            let mut stdout = BufReader::new(stdout);
            loop {
                let line = read_line(&mut stdout).await;
                let next = match line {
                    Ok(line) if probe => Ok(line),
                    Ok(line) => match protocol::decode(&line) {
                        Ok(Frame::Status(status)) => {
                            status_send.send_replace(Some(status));
                            continue;
                        }
                        Ok(Frame::Reply { .. }) => Ok(line),
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                };
                let failed = next.is_err();
                if failed {
                    failure.cancel();
                }
                if send.send(next).await.is_err() || failed {
                    break;
                }
            }
        });
        let diagnostics = tokio::spawn(async move {
            let mut scratch = [0; 4096];
            // Fixed storage, no accumulation, no logging, including on spawn/command failures.
            while let Ok(n) = stderr.read(&mut scratch).await {
                if n == 0 {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            _artifact: executable.clone(),
            stdin: Some(stdin),
            frames,
            statuses,
            tasks: vec![reader, diagnostics],
            next_id: 0,
            failed: false,
            exit_confirmed: false,
            kill_requested: false,
            stop_attempted: false,
            stop_acknowledged: false,
            stopped,
        })
    }
    pub(crate) async fn command(
        &mut self,
        method: &str,
        payload: Value,
        timeout: Duration,
    ) -> Result<Value, HelperError> {
        self.command_inner(method, payload, timeout, true).await
    }
    async fn command_inner(
        &mut self,
        method: &str,
        payload: Value,
        timeout: Duration,
        terminate_on_error: bool,
    ) -> Result<Value, HelperError> {
        if self.failed {
            return Err(HelperError::Stopped);
        }
        self.next_id += 1;
        let id = self.next_id.to_string();
        let mut line = serde_json::to_vec(&json!({"id":id,"method":method,"payload":payload}))
            .map_err(|_| HelperError::Protocol)?;
        if line.len() > protocol::COMMAND_LIMIT {
            return Err(HelperError::Configuration);
        }
        line.push(b'\n');
        let result = tokio::time::timeout(timeout, async {
            self.stdin
                .as_mut()
                .ok_or(HelperError::Stopped)?
                .write_all(&line)
                .await
                .map_err(|_| HelperError::Stopped)?;
            self.stdin
                .as_mut()
                .ok_or(HelperError::Stopped)?
                .flush()
                .await
                .map_err(|_| HelperError::Stopped)?;
            let line = self.frames.recv().await.ok_or(HelperError::Stopped)??;
            match protocol::decode(&line)? {
                Frame::Reply {
                    id: reply_id,
                    value,
                } if reply_id == id => value,
                _ => Err(HelperError::Protocol),
            }
        })
        .await
        .map_err(|_| HelperError::Deadline)
        .and_then(|r| r);
        if result.is_err() && terminate_on_error {
            self.terminate().await?;
        }
        result
    }
    pub(crate) fn poll_exit(&mut self) -> Result<bool, HelperError> {
        if self.exit_confirmed {
            return Ok(true);
        }
        if self
            .child
            .try_wait()
            .map_err(|_| HelperError::Stopped)?
            .is_some()
        {
            self.exit_confirmed = true;
            self.failed = true;
            self.stdin.take();
            self.stopped.cancel();
            for task in &self.tasks {
                task.abort();
            }
        }
        Ok(self.exit_confirmed)
    }
    async fn wait_exit(&mut self) -> Result<(), HelperError> {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if self.poll_exit()? {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(HelperError::Deadline);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    pub(crate) fn request_termination(&mut self) -> Result<(), HelperError> {
        self.failed = true;
        self.stopped.cancel();
        self.stdin.take();
        if self.poll_exit()? {
            return Ok(());
        }
        if !self.kill_requested {
            self.child.start_kill().map_err(|_| HelperError::Stopped)?;
            self.kill_requested = true;
        }
        Ok(())
    }
    pub(crate) async fn terminate(&mut self) -> Result<(), HelperError> {
        self.request_termination()?;
        self.wait_exit().await
    }
    pub(crate) async fn stop_confirmed(&mut self) -> Result<bool, HelperError> {
        if !self.stop_attempted {
            self.stop_attempted = true;
            if !self.failed {
                self.stop_acknowledged = self
                    .command_inner("stop", json!({}), Duration::from_secs(2), false)
                    .await
                    .is_ok_and(|v| v == json!({"stopped":true}));
            }
        }
        self.stdin.take();
        self.failed = true;
        self.stopped.cancel();
        self.wait_exit().await?;
        Ok(self.stop_acknowledged)
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
async fn read_line<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Vec<u8>, HelperError> {
    let mut line = Vec::new();
    loop {
        let part = reader.fill_buf().await.map_err(|_| HelperError::Stopped)?;
        if part.is_empty() {
            return Err(HelperError::Stopped);
        }
        let end = part.iter().position(|b| *b == b'\n');
        let count = end.map_or(part.len(), |n| n + 1);
        if line.len() + count > protocol::LINE_LIMIT {
            return Err(HelperError::Protocol);
        }
        line.extend_from_slice(&part[..count]);
        reader.consume(count);
        if end.is_some() {
            line.pop();
            return Ok(line);
        }
    }
}
