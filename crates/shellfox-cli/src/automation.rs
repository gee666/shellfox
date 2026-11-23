use crate::automation_args::AutomationCommand;
use anyhow::{bail, Context, Result};
use shellfox_service::local::{automation::*, LocalClient, LocalRequestFailure};
use std::{
    future::Future,
    io::{Read, Write},
    pin::Pin,
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

pub async fn run(client: &LocalClient, command: AutomationCommand) -> Result<i32> {
    match command {
        AutomationCommand::List => {
            crate::commands::print_json(&client.request(&AutomationList {}).await?.0)?;
            Ok(0)
        }
        AutomationCommand::Show(id) => {
            crate::commands::print_json(&client.request(&AutomationShow { id }).await?)?;
            Ok(0)
        }
        AutomationCommand::Stop(id) => {
            crate::commands::print_json(&client.request(&AutomationStop { id }).await?)?;
            Ok(0)
        }
        AutomationCommand::Run(spec) => {
            let detached = spec.detached;
            // No stdin read before containment and the real Start READY reply.
            let run = client.request(&*spec).await?;
            if detached {
                crate::commands::print_json(&run)?;
                return Ok(0);
            }
            foreground(client, run.id).await
        }
    }
}
async fn foreground(client: &LocalClient, id: Uuid) -> Result<i32> {
    foreground_with_interrupt(client, id, stdin_reader()?, async {
        Ok(tokio::signal::ctrl_c().await?)
    })
    .await
}
type PendingWrite<'a> = Pin<Box<dyn Future<Output = Result<AutomationAccepted>> + 'a>>;
fn stdin_closed(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<LocalRequestFailure>()
        .is_some_and(LocalRequestFailure::automation_stdin_closed)
}
async fn foreground_with_interrupt<F: Future<Output = Result<()>>>(
    client: &LocalClient,
    id: Uuid,
    mut input: mpsc::Receiver<Result<Vec<u8>>>,
    interrupt: F,
) -> Result<i32> {
    let output = pipe_output(client, id, output_writer()?);
    tokio::pin!(output, interrupt);
    let mut write: Option<PendingWrite<'_>> = None;
    let mut input_ended = false;
    let mut final_output = None;
    loop {
        tokio::select! {
            biased;
            result=async {match write.as_mut() {Some(write)=>write.await,None=>std::future::pending().await}}=>{
                write=None;
                if let Err(error)=result {
                    if !stdin_closed(&error) {return Err(error);}
                    // A known pipe closure is provisional until the same run's
                    // actual final DTO arrives; never replay the input.
                    input_ended=true;
                }
                if let Some(code)=final_output {return Ok(code);}
            },
            bytes=input.recv(),if !input_ended && write.is_none() && final_output.is_none()=>{
                let bytes=bytes.context("CLI stdin reader stopped before EOF.")??;
                let eof=bytes.is_empty();input_ended=eof;
                let request=AutomationWrite {id,data:shellfox_service::local::encode_chunk(&bytes)?,eof};
                write=Some(Box::pin(async move {client.request(&request).await}));
            },
            result=&mut output,if final_output.is_none()=>{
                final_output=Some(result?);input_ended=true;
                // Await exactly one accepted/in-flight write, not another read
                // from interactive stdin. Unknown write failures remain errors.
                if write.is_none() {return Ok(final_output.expect("just assigned"));}
            },
            interrupted=&mut interrupt=>{
                interrupted?;
                client.request(&AutomationStop {id}).await?;
                if let Some(write)=write.take() {
                    if let Err(error)=write.await {if !stdin_closed(&error) {return Err(error);}}
                }
                let run=client.request(&AutomationList {}).await?.0.into_iter().find(|run|run.id==id).context("Stopped run receipt is unavailable; no exit status was inferred.")?;
                if run.state!=AutomationState::Exited {bail!("Stop did not yield the correlated final run outcome.");}
                return Ok(exit_code(run.exit_code,run.signal.as_deref()));
            }
        }
    }
}
fn stdin_reader() -> Result<mpsc::Receiver<Result<Vec<u8>>>> {
    let (send, receive) = mpsc::channel(2);
    std::thread::Builder::new()
        .name("shellfox-cli-raw-stdin".into())
        .spawn(move || {
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            loop {
                let mut bytes = vec![0; AUTOMATION_CHUNK_BYTES];
                match input.read(&mut bytes) {
                    Ok(count) => {
                        bytes.truncate(count);
                        if send.blocking_send(Ok(bytes)).is_err() || count == 0 {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        let _ = send.blocking_send(Err(error.into()));
                        break;
                    }
                }
            }
        })?;
    // A blocked interactive reader owns no process and ends with this CLI.
    Ok(receive)
}
struct OutputBatch {
    frames: Vec<(AutomationStream, Vec<u8>)>,
    reply: oneshot::Sender<Result<()>>,
}
fn output_writer() -> Result<mpsc::Sender<OutputBatch>> {
    let (send, mut receive) = mpsc::channel::<OutputBatch>(1);
    std::thread::Builder::new()
        .name("shellfox-cli-raw-output".into())
        .spawn(move || {
            while let Some(batch) = receive.blocking_recv() {
                let result = write_batch(batch.frames);
                let failed = result.is_err();
                let _ = batch.reply.send(result);
                if failed {
                    break;
                }
            }
        })?;
    Ok(send)
}
fn write_batch(frames: Vec<(AutomationStream, Vec<u8>)>) -> Result<()> {
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    for (stream, bytes) in frames {
        match stream {
            AutomationStream::Stdout => {
                let mut output = stdout.lock();
                output.write_all(&bytes)?;
                output.flush()?;
            }
            AutomationStream::Stderr => {
                let mut output = stderr.lock();
                output.write_all(&bytes)?;
                output.flush()?;
            }
        }
    }
    Ok(())
}
fn exit_code(code: Option<i64>, signal: Option<&str>) -> i32 {
    if let Some(code) = code {
        #[cfg(windows)]
        return u32::try_from(code).map(|status| status as i32).unwrap_or(1);
        #[cfg(not(windows))]
        return i32::try_from(code).unwrap_or(1);
    }
    // Legacy display/CLI convention, never OS-signal or cleanup evidence.
    match signal {
        Some("SIGINT") => 130,
        Some("SIGTERM") => 143,
        _ => 137,
    }
}
async fn pipe_output(
    client: &LocalClient,
    id: Uuid,
    output: mpsc::Sender<OutputBatch>,
) -> Result<i32> {
    let mut after = 0;
    loop {
        let result = client.request(&AutomationRead { id, after }).await?;
        let empty = result.frames.is_empty();
        if !empty {
            let mut frames = Vec::with_capacity(result.frames.len());
            let mut next = after;
            for frame in result.frames {
                if frame.sequence <= next {
                    bail!("Invalid automation output cursor.");
                }
                next = frame.sequence;
                frames.push((
                    frame.stream,
                    shellfox_service::local::decode_chunk(&frame.data)?,
                ));
            }
            let (reply, written) = oneshot::channel();
            output
                .send(OutputBatch { frames, reply })
                .await
                .map_err(|_| anyhow::anyhow!("CLI output writer stopped."))?;
            written
                .await
                .context("CLI output acknowledgement lost.")??;
            after = next; // Only after actual destination writes/flushes.
        }
        if result.run.state == AutomationState::Exited && empty {
            return Ok(exit_code(
                result.run.exit_code,
                result.run.signal.as_deref(),
            ));
        }
    }
}
#[cfg(test)]
#[path = "automation_control_tests.rs"]
mod control_tests;
#[cfg(all(test, windows))]
#[path = "../tests/support/owned.rs"]
pub(crate) mod test_owned;
#[cfg(test)]
mod tests {
    use super::{exit_code, stdin_closed, LocalRequestFailure};
    #[test]
    fn only_exact_typed_automation_write_closure_can_be_correlated() {
        let failure = |method, code| {
            anyhow::Error::new(LocalRequestFailure {
                method,
                error: serde_json::from_value(
                    serde_json::json!({"code":code,"message":"fixture","retryable":false}),
                )
                .unwrap(),
            })
        };
        assert!(stdin_closed(&failure("automation.write", "TARGET_LOST")));
        for code in ["AUTH_FAILED", "VALIDATION", "INTERNAL", "ACCESS_DENIED"] {
            assert!(!stdin_closed(&failure("automation.write", code)));
        }
        assert!(!stdin_closed(&failure("automation.read", "TARGET_LOST")));
        assert!(!stdin_closed(&anyhow::anyhow!(
            "Automation stdin is closed."
        )));
    }
    #[test]
    fn natural_exit_wins_and_signal_dto_uses_existing_cli_convention() {
        assert_eq!(exit_code(Some(7), None), 7);
        assert_eq!(exit_code(Some(7), Some("SIGKILL")), 7);
        assert_eq!(exit_code(None, Some("SIGKILL")), 137);
        assert_eq!(exit_code(None, Some("SIGINT")), 130);
        assert_eq!(exit_code(None, Some("SIGTERM")), 143);
        assert_eq!(exit_code(None, None), 137);
        #[cfg(windows)]
        {
            assert_eq!(exit_code(Some(0xffff_ffff), None), -1);
            assert_eq!(exit_code(Some(0x8000_0000), None), i32::MIN);
            assert_eq!(exit_code(Some(0xc000_013a), None) as u32, 0xc000_013a);
        }
        #[cfg(not(windows))]
        assert_eq!(exit_code(Some(-1), None), -1);
    }
}
