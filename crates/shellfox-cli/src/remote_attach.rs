//! `terminals attach --host <peer>`: the 0.9 attach (src/cli/attach.ts) over one
//! v1 view. Output streams from the peer's event poll; keys and pastes are
//! encoded by a local Ghostty terminal that follows the remote modes, as the
//! SSH client does. Input is never retried, and only the owner may write.
//! A peer that stops answering is re-paired with the client's backoff and
//! the screen replayed; input typed meanwhile is dropped.
use crate::{
    keys::{Encoded, Encoder},
    output::CliError,
    queries::QueryFilter,
};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::{Stream, StreamExt};
use shellfox_client::{Client, ConnectionStatus, EventDisposition, ViewCursor};
use shellfox_core::TabId;
use shellfox_protocol::{
    AcknowledgeTerminal, AttachTerminal, DetachTerminal, Generation, HostEvent, OutboundTransport,
    Reply, Request, TakeTerminalControl, TerminalAttachment, TerminalEvent,
};
use std::{cell::RefCell, io::Write};

const BLOCKED: &str = "\x1b[2J\x1b[HAnother client is controlling this terminal.\r\n\r\nCtrl+T: Take it    Ctrl+]: Detach\r\n";
const INPUT_CHUNK: usize = 16 * 1024;

struct View<W> {
    output: W,
    cursor: Option<ViewCursor>,
    generation: Option<Generation>,
    owned: bool,
    encoder: Encoder,
    queries: QueryFilter,
    size: (u16, u16),
}
impl<W: Write> View<W> {
    fn show(&mut self, bytes: &[u8], historical: bool) -> Result<()> {
        let visible = self.queries.push(bytes, historical);
        self.output.write_all(&visible)?;
        self.output.flush()?;
        self.encoder.feed(bytes)
    }
    fn blocked(&mut self) -> Result<()> {
        self.owned = false;
        self.queries.reset();
        self.output.write_all(crate::presentation::RESET)?;
        self.output.write_all(BLOCKED.as_bytes())?;
        Ok(self.output.flush()?)
    }
}

/// What a terminal event means for this attach.
enum Next {
    Continue,
    Reattach,
    Finished,
}

struct Attach<'a, T: OutboundTransport, W> {
    client: &'a Client<T>,
    tab: TabId,
    view: RefCell<View<W>>,
}

fn value<V>(reply: Reply<V>) -> Result<V> {
    match reply {
        Reply::Success(value) => Ok(value),
        Reply::Failure(error) => Err(CliError::app(error)),
    }
}
fn lost() -> anyhow::Error {
    CliError::coded(
        "TARGET_LOST",
        "The remote host did not answer. Input was not retried; inspect the terminal before sending it again.",
        false,
    )
}
fn control_required(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<CliError>()
        .is_some_and(|e| e.code == "TERMINAL_CONTROL_REQUIRED")
}

impl<T: OutboundTransport, W: Write> Attach<'_, T, W> {
    fn connection(&self) -> u64 {
        self.client.state().connection_id
    }
    /// A failed request that also dropped the connection: the peer (or the
    /// local host carrying its traffic) stopped answering.
    fn disconnected(&self) -> bool {
        self.client.state().status != ConnectionStatus::Connected
    }
    /// Pairs again until the peer answers, waiting as the client advises.
    /// A peer that rejects this client ends the attach.
    async fn reconnect(&self) -> Result<()> {
        loop {
            let state = self.client.state();
            if state.status == ConnectionStatus::Connected {
                return Ok(());
            }
            let Some(delay) = state.retry_after else {
                return Err(state.last_error.map_or_else(lost, CliError::app));
            };
            tokio::time::sleep(delay).await;
            let _ = self.client.pair().await;
        }
    }
    async fn send<R: Request>(&self, request: &R) -> Result<R::Response> {
        value(self.client.execute(request).await.map_err(|_| lost())?)
    }
    /// A request only the owner may make. Losing control shows the blocked screen.
    async fn owned<R: Request>(&self, request: Option<R>) -> Result<()> {
        let Some(request) = request else {
            return Ok(());
        };
        match self.send(&request).await {
            Err(error) if control_required(&error) => self.view.borrow_mut().blocked(),
            // Not retried: the output stream reconnects and replays the screen.
            Err(_) if self.disconnected() => Ok(()),
            result => result.map(|_| ()),
        }
    }

    /// A fresh attach replays the screen and becomes owner, or shows who holds it.
    async fn attach(&self) -> Result<()> {
        let generation = self.view.borrow().generation;
        let attachment = self
            .send(&AttachTerminal {
                tab_id: self.tab,
                after_sequence: None,
                generation,
            })
            .await?;
        let connection = self.connection();
        let mut cursor = ViewCursor::from_attachment(&attachment, connection)
            .map_err(|_| anyhow::anyhow!("The remote host returned an invalid attachment."))?;
        let (ack, resize) = {
            let mut view = self.view.borrow_mut();
            match attachment {
                TerminalAttachment::Blocked { generation, .. } => {
                    view.generation = Some(generation);
                    view.cursor = Some(cursor);
                    view.blocked()?;
                    return Ok(());
                }
                TerminalAttachment::Owned(owned) => {
                    view.generation = Some(owned.generation);
                    view.owned = true;
                    view.queries.reset();
                    view.output.write_all(crate::presentation::RESET)?;
                    view.output.write_all(b"\x1b[2J\x1b[H")?;
                    view.encoder.resize(owned.cols, owned.rows)?;
                    for chunk in &owned.chunks {
                        view.show(chunk.data.as_bytes(), true)?;
                    }
                    let ack = cursor.consumed(owned.last_sequence, connection).ok();
                    let (cols, rows) = view.size;
                    view.encoder.resize(cols, rows)?;
                    let resize = cursor.resize(cols, rows, connection).ok();
                    view.cursor = Some(cursor);
                    (ack.filter(|_| owned.last_sequence > 0), resize)
                }
            }
        };
        self.owned(ack).await?;
        self.owned(resize).await
    }

    fn observe(&self, event: &TerminalEvent) -> Result<(Next, Option<AcknowledgeTerminal>)> {
        let connection = self.connection();
        let mut view = self.view.borrow_mut();
        if let TerminalEvent::Exit {
            tab_id, generation, ..
        } = event
        {
            if *tab_id == self.tab && Some(*generation) == view.generation && !view.owned {
                return Ok((Next::Finished, None));
            }
        }
        let Some(mut cursor) = view.cursor.take() else {
            return Ok((Next::Continue, None));
        };
        let disposition = cursor.observe(event, connection);
        let ack = match (&disposition, event) {
            (Ok(EventDisposition::Deliver), TerminalEvent::Data { sequence, .. }) => {
                cursor.consumed(*sequence, connection).ok()
            }
            _ => None,
        };
        view.cursor = Some(cursor);
        let disposition = disposition
            .map_err(|_| anyhow::anyhow!("The remote host sent an invalid terminal event."))?;
        Ok(match (disposition, event) {
            (EventDisposition::Deliver, TerminalEvent::Data { data, .. }) => {
                view.show(data.as_bytes(), false)?;
                (Next::Continue, ack)
            }
            (EventDisposition::Deliver, TerminalEvent::Exit { .. }) => (Next::Finished, None),
            (EventDisposition::Deliver, TerminalEvent::Error { error, .. }) => {
                return Err(CliError::app(error.clone()))
            }
            (EventDisposition::Reattach, _) => (Next::Reattach, None),
            (EventDisposition::Blocked, _) => {
                view.blocked()?;
                (Next::Continue, None)
            }
            _ => (Next::Continue, None),
        })
    }

    /// Streams the peer's terminal events until the shell exits.
    async fn output(&self) -> Result<()> {
        loop {
            let events = match self.client.poll().await {
                Ok(Reply::Success(response)) => response.events,
                _ if self.disconnected() => {
                    self.resume().await?;
                    continue;
                }
                Ok(Reply::Failure(error)) => return Err(CliError::app(error)),
                Err(_) => return Err(lost()),
            };
            for event in events {
                let HostEvent::Terminal { event } = event else {
                    continue;
                };
                let (next, ack) = self.observe(&event)?;
                self.owned(ack).await?;
                match next {
                    Next::Continue => {}
                    Next::Reattach => self.attach().await?,
                    Next::Finished => return Ok(()),
                }
            }
        }
    }

    /// Reconnects and attaches again, until a fresh attach replays the screen.
    async fn resume(&self) -> Result<()> {
        loop {
            self.reconnect().await?;
            match self.attach().await {
                Err(_) if self.disconnected() => continue,
                result => return result,
            }
        }
    }

    async fn write(&self, bytes: Vec<u8>) -> Result<()> {
        let text =
            String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("Encoded input is not UTF-8."))?;
        let mut rest = text.as_str();
        while !rest.is_empty() {
            let mut end = rest.len().min(INPUT_CHUNK);
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            let (chunk, tail) = rest.split_at(end);
            rest = tail;
            let request = {
                let view = self.view.borrow();
                if !view.owned {
                    return Ok(());
                }
                view.cursor.as_ref().and_then(|c| {
                    c.write(chunk.to_owned(), Some(true), self.connection())
                        .ok()
                })
            };
            self.owned(request).await?;
        }
        Ok(())
    }

    /// Returns false once the user detaches.
    async fn input(&self, event: Event) -> Result<bool> {
        if let Event::Resize(cols, rows) = event {
            let size = (cols.clamp(2, 500), rows.clamp(2, 500));
            let request = {
                let mut view = self.view.borrow_mut();
                view.size = size;
                view.encoder.resize(size.0, size.1)?;
                let owned = view.owned;
                view.cursor
                    .as_ref()
                    .filter(|_| owned)
                    .and_then(|c| c.resize(size.0, size.1, self.connection()).ok())
            };
            self.owned(request).await?;
            return Ok(true);
        }
        if let Event::Key(key) = &event {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            let blocked = !self.view.borrow().owned;
            if ctrl
                && key.code == KeyCode::Char('t')
                && key.kind != KeyEventKind::Release
                && blocked
            {
                let generation = self.view.borrow().generation;
                if let Some(generation) = generation {
                    self.send(&TakeTerminalControl {
                        tab_id: self.tab,
                        generation,
                    })
                    .await?;
                }
                self.attach().await?;
                return Ok(true);
            }
        }
        let encoded = self.view.borrow_mut().encoder.input(event)?;
        match encoded {
            Encoded::Detach => return Ok(false),
            Encoded::Bytes(bytes) => self.write(bytes).await?,
            Encoded::Ignore => {}
        }
        Ok(true)
    }
}

/// Attaches `tab` on the paired peer until Ctrl+], the shell exits or input ends.
/// Detaching never closes the remote shell.
pub async fn run<T, W, S>(
    client: &Client<T>,
    tab: TabId,
    size: (u16, u16),
    output: W,
    mut input: S,
) -> Result<()>
where
    T: OutboundTransport,
    W: Write,
    S: Stream<Item = std::io::Result<Event>> + Unpin,
{
    let size = (size.0.clamp(2, 500), size.1.clamp(2, 500));
    let attach = Attach {
        client,
        tab,
        view: RefCell::new(View {
            output,
            cursor: None,
            generation: None,
            owned: false,
            encoder: Encoder::attach(size.0, size.1)?,
            queries: QueryFilter::default(),
            size,
        }),
    };
    attach.attach().await?;
    let keyboard = async {
        while let Some(event) = input.next().await {
            if !attach.input(event?).await? {
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    };
    // Output keeps streaming while a paste waits for its reply.
    let result = tokio::select! {
        result = attach.output() => result,
        result = keyboard => result,
    };
    let generation = attach.view.borrow().generation;
    if let Some(generation) = generation {
        let _ = client
            .execute(&DetachTerminal {
                tab_id: tab,
                generation,
            })
            .await;
    }
    result
}

#[cfg(test)]
#[path = "remote_attach_tests.rs"]
mod tests;
