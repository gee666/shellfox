use super::{
    session::ConnectionError,
    terminal::Terminal,
};
use anyhow::Result;
use russh::{client, Channel, ChannelMsg, Sig};
use std::io::{self, Read, Write};
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
pub(super) async fn exchange(channel: Channel<client::Msg>, terminal: &Terminal) -> Result<i32> {
    let (mut read, write) = channel.split();
    let remote = async {
        let mut exit = None;
        while let Some(message) = read.wait().await {
            match message {
                ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => output(&data)?,
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
        forward_input(&write, terminal).await?;
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
) -> Result<()> {
    use crate::keys::{self, Input};
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
        match terminal.next_event().await? {
            Event::Resize(cols, rows) => {
                write.window_change(cols.into(), rows.into(), 0, 0).await?
            }
            Event::Paste(text) => {
                write
                    .data_bytes(format!("\x1b[200~{text}\x1b[201~"))
                    .await?
            }
            event => match keys::from_event(event) {
                Input::Bytes(bytes) => write.data_bytes(bytes).await?,
                Input::Detach => write.data_bytes(vec![29]).await?,
                Input::Ignore => {}
            },
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
