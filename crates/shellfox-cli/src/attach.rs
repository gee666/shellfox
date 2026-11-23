use crate::{
    keys::{self, Input},
    queries::QueryFilter,
};
use anyhow::{bail, Result};
use crossterm::{
    event::{Event, EventStream},
    terminal,
};
use futures_util::StreamExt;
use shellfox_core::TabId;
use shellfox_service::{
    experiment::{ExperimentRequest, ExperimentResponse, RecoveryCapability},
    local::LocalClient,
    Attachment, TerminalOutput,
};
use std::io::{self, Write};

const PASTE_CHUNK: usize = 16 * 1024;

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
    let client = &*client;
    let mut events = EventStream::new();
    let mut keys = keys::Keys::default();
    // The host holds each output read until the terminal has something new,
    // so keys and output are awaited together instead of polled.
    let poll = || {
        client.execute(ExperimentRequest::Poll {
            tab,
            attachment: attachment.clone(),
        })
    };
    let mut output = None;
    loop {
        // A new read starts only after the previous reply was handled, so it
        // never overtakes a recovery.
        let pending = output.get_or_insert_with(|| Box::pin(poll()));
        let reply = tokio::select! {
            event = events.next() => {
                let Some(event) = event else { return Ok(()) };
                let event = event?;
                if let Event::Resize(cols, rows) = event {
                    resize(client, tab, &attachment, cols, rows).await?;
                    continue;
                }
                // The host's terminal encodes keys and pastes with the modes the
                // application set, even ones recovery could not replay locally.
                let attachment = attachment.clone();
                let request = match keys.input(event) {
                    Input::Detach => return Ok(()),
                    Input::Key(event) => ExperimentRequest::Key { tab, attachment, event },
                    Input::Paste(text) => {
                        // Pastes above the host's 64 KiB input bound go in pieces.
                        for text in shellfox_ghostty_vt::paste_chunks(&text, PASTE_CHUNK) {
                            let attachment = attachment.clone();
                            let paste = ExperimentRequest::Paste { tab, attachment, text: text.to_owned() };
                            done(client.execute(paste).await?)?;
                        }
                        continue;
                    }
                    Input::Ignore => continue,
                };
                done(client.execute(request).await?)?;
                continue;
            }
            reply = pending => reply?,
        };
        output = None;
        let mut chunks = match reply {
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
    }
    // Dropping the client releases the attachment. Never send CloseTab or Stop.
}

async fn resize(
    client: &LocalClient,
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
    client: &LocalClient,
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
    client: &LocalClient,
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
