use crate::{
    keys::{self, Input},
    queries::QueryFilter,
};
use anyhow::{bail, Result};
use crossterm::{
    event::{self, Event},
    terminal,
};
use shellfox_core::TabId;
use shellfox_service::{
    experiment::{ExperimentRequest, ExperimentResponse, RecoveryCapability},
    local::LocalClient,
    Attachment, TerminalOutput,
};
use std::{
    io::{self, Write},
    time::Duration,
};

const INPUT_CHUNK: usize = 16 * 1024;

#[path = "terminal_guard.rs"]
mod guard;
pub(crate) use guard::{require_terminal, TerminalGuard};

pub async fn run(client: &mut LocalClient, tab: TabId, takeover: bool) -> Result<()> {
    require_terminal()?;
    eprintln!(
        "Recovery restores the visible screen only, not shell/TUI modes or scrollback history."
    );
    let _terminal = TerminalGuard::enter()?;
    let attachment = match client
        .execute(ExperimentRequest::Attach { tab, takeover })
        .await?
    {
        ExperimentResponse::Attachment(attachment) => attachment,
        _ => bail!("Unexpected service attach response."),
    };
    let (cols, rows) = terminal::size()?;
    resize(client, tab, &attachment, cols, rows).await?;
    let mut queries = QueryFilter::default();
    let mut last_sequence = recover(client, tab, &attachment, &mut queries).await?;
    loop {
        // A bounded batch prevents keyboard repeat from starving output polling.
        for _ in 0..64 {
            if !event::poll(Duration::ZERO)? {
                break;
            }
            let event = event::read()?;
            if let Event::Resize(cols, rows) = event {
                resize(client, tab, &attachment, cols, rows).await?;
                continue;
            }
            let bytes = match keys::from_event(event) {
                Input::Detach => return Ok(()),
                Input::Bytes(bytes) => bytes,
                Input::Ignore => continue,
            };
            for bytes in bytes.chunks(INPUT_CHUNK) {
                done(
                    client
                        .execute(ExperimentRequest::Input {
                            tab,
                            attachment: attachment.clone(),
                            bytes: bytes.to_vec(),
                        })
                        .await?,
                )?;
            }
        }
        let mut chunks = match client
            .execute(ExperimentRequest::Poll {
                tab,
                attachment: attachment.clone(),
            })
            .await?
        {
            ExperimentResponse::Output(chunks) => chunks,
            ExperimentResponse::RecoveryRequired {} => {
                last_sequence = recover(client, tab, &attachment, &mut queries).await?;
                continue;
            }
            ExperimentResponse::TerminalExited {
                last_sequence: final_sequence,
            } => {
                if final_sequence != last_sequence {
                    bail!("Terminal exited before all output was confirmed.");
                }
                return Ok(());
            }
            _ => bail!("Unexpected service output response."),
        };
        // Poll can repeat unacknowledged chunks after a lost reply.
        chunks.retain(|chunk| chunk.sequence > last_sequence);
        if !is_contiguous(Some(last_sequence), &chunks) {
            // Never append across a known gap. Recover replaces the visible screen.
            last_sequence = recover(client, tab, &attachment, &mut queries).await?;
        } else if let Some(sequence) = chunks.last().map(|chunk| chunk.sequence) {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            for chunk in chunks {
                output.write_all(&queries.push(&chunk.bytes, false))?;
            }
            output.flush()?;
            drop(output);
            acknowledge(client, tab, &attachment, sequence).await?;
            last_sequence = sequence;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Dropping the client releases the attachment. Never send CloseTab or Stop.
}

async fn resize(
    client: &mut LocalClient,
    tab: TabId,
    attachment: &Attachment,
    cols: u16,
    rows: u16,
) -> Result<()> {
    if cols == 0 || rows == 0 {
        return Ok(());
    }
    done(
        client
            .execute(ExperimentRequest::Resize {
                tab,
                attachment: attachment.clone(),
                cols,
                rows,
            })
            .await?,
    )
}

async fn recover(
    client: &mut LocalClient,
    tab: TabId,
    attachment: &Attachment,
    queries: &mut QueryFilter,
) -> Result<u64> {
    match client
        .execute(ExperimentRequest::Recover {
            tab,
            attachment: attachment.clone(),
        })
        .await?
    {
        ExperimentResponse::Recovery {
            bytes,
            last_sequence,
            capability: RecoveryCapability::VisibleScreen,
        } => {
            queries.reset();
            let mut output = io::stdout().lock();
            output.write_all(&queries.push(&bytes, true))?;
            output.flush()?;
            drop(output);
            acknowledge(client, tab, attachment, last_sequence).await?;
            Ok(last_sequence)
        }
        _ => bail!("Unexpected service recovery response."),
    }
}

async fn acknowledge(
    client: &mut LocalClient,
    tab: TabId,
    attachment: &Attachment,
    sequence: u64,
) -> Result<()> {
    done(
        client
            .execute(ExperimentRequest::Acknowledge {
                tab,
                attachment: attachment.clone(),
                sequence,
            })
            .await?,
    )
}

fn done(response: ExperimentResponse) -> Result<()> {
    match response {
        ExperimentResponse::Done {} => Ok(()),
        _ => bail!("Unexpected service terminal command response."),
    }
}

fn is_contiguous(mut previous: Option<u64>, chunks: &[TerminalOutput]) -> bool {
    for chunk in chunks {
        if previous.is_some_and(|value| value.checked_add(1) != Some(chunk.sequence)) {
            return false;
        }
        previous = Some(chunk.sequence);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn output(sequence: u64) -> TerminalOutput {
        TerminalOutput {
            sequence,
            bytes: vec![],
        }
    }
    #[test]
    fn sequence_gaps_duplicates_reordering_and_overflow_need_recovery() {
        assert!(is_contiguous(None, &[]));
        assert!(is_contiguous(None, &[output(42), output(43)]));
        assert!(is_contiguous(Some(43), &[output(44)]));
        assert!(!is_contiguous(Some(43), &[output(45)]));
        assert!(!is_contiguous(None, &[output(42), output(42)]));
        assert!(!is_contiguous(None, &[output(42), output(41)]));
        assert!(!is_contiguous(Some(u64::MAX), &[output(0)]));
    }
}
