use super::{session::ConnectionError, terminal::Terminal};
use crate::keys::{Encoded, Encoder};
use anyhow::Result;
use russh::{client, Channel, ChannelMsg, Sig};
use std::io::{self, Read, Write};
use std::sync::Mutex;
use tokio::sync::mpsc;

pub(super) fn output(bytes: &[u8]) -> Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(bytes)?;
    stdout.flush()?;
    Ok(())
}
fn redirected_input() -> Result<mpsc::Receiver<io::Result<Vec<u8>>>> {
    let (send, recv) = mpsc::channel(8);
    std::thread::Builder::new()
        .name("shellfox-ssh-input".into())
        .spawn(move || {
            let mut stdin = io::stdin().lock();
            let mut buf = [0u8; 8192];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if send.blocking_send(Ok(buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        let _ = send.blocking_send(Err(e));
                        break;
                    }
                }
            }
        })?;
    Ok(recv)
}
/// The encoder, shared by the output and input halves of one exchange.
struct Mirror(Mutex<Encoder>);
impl Mirror {
    fn with<T>(&self, f: impl FnOnce(&mut Encoder) -> Result<T>) -> Result<T> {
        f(&mut *self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal state lock poisoned"))?)
    }
}
pub(super) async fn exchange(channel: Channel<client::Msg>, terminal: &Terminal) -> Result<i32> {
    let (mut read, write) = channel.split();
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let mirror = Mirror(Mutex::new(Encoder::ssh(cols, rows)?));
    let remote = async {
        let mut exit = None;
        while let Some(message) = read.wait().await {
            match message {
                ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                    output(&data)?;
                    mirror.with(|m| m.feed(&data))?;
                }
                ChannelMsg::ExitStatus { exit_status } => exit = Some(exit_status as i32),
                ChannelMsg::ExitSignal { signal_name, .. } => {
                    exit = Some(signal_status(signal_name))
                }
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        exit.ok_or_else(|| ConnectionError("connection lost".into()).into())
    };
    let local = async {
        forward_input(&write, terminal, &mirror).await?;
        std::future::pending().await
    };
    // A blocked paste must not stop us from consuming remote output or exit.
    tokio::select! {
        biased;
        result = remote => result,
        result = local => result,
    }
}

async fn forward_input(
    write: &russh::ChannelWriteHalf<client::Msg>,
    terminal: &Terminal,
    mirror: &Mirror,
) -> Result<()> {
    use crossterm::event::Event;
    if !terminal.tty {
        let mut input = redirected_input()?;
        while let Some(item) = input.recv().await {
            write.data_bytes(item?).await?;
        }
        write.eof().await?;
        return Ok(());
    }
    loop {
        let event = terminal.next_event().await?;
        if let Event::Resize(cols, rows) = event {
            mirror.with(|m| m.resize(cols, rows))?;
            write.window_change(cols.into(), rows.into(), 0, 0).await?;
        } else if let Encoded::Bytes(bytes) = mirror.with(|m| m.input(event))? {
            write.data_bytes(bytes).await?;
        }
    }
}
fn signal_status(signal: Sig) -> i32 {
    128 + match signal {
        Sig::HUP => 1,
        Sig::INT => 2,
        Sig::QUIT => 3,
        Sig::ILL => 4,
        Sig::ABRT => 6,
        Sig::FPE => 8,
        Sig::KILL => 9,
        Sig::SEGV => 11,
        Sig::PIPE => 13,
        Sig::ALRM => 14,
        Sig::TERM => 15,
        _ => 0,
    }
}
