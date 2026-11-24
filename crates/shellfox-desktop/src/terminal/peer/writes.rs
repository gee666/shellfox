//! Input waiting for the remote host. One write is in flight at a time;
//! the rest join so a burst of typing goes out in a few requests.
use shellfox_protocol::WriteTerminal;
use std::collections::VecDeque;

/// Typing pauses while this much input waits for the remote host.
const MAX_PENDING_BYTES: usize = 1024 * 1024;
/// The v1 protocol's bound on one WriteTerminal.
const MAX_WRITE_BYTES: usize = 65536;

pub(super) fn push(
    writes: &mut VecDeque<WriteTerminal>,
    write: WriteTerminal,
) -> anyhow::Result<()> {
    let pending: usize = writes.iter().map(|write| write.data.len()).sum();
    if pending + write.data.len() > MAX_PENDING_BYTES {
        anyhow::bail!(crate::terminal::runtime::PAUSED);
    }
    match writes.back_mut() {
        Some(tail)
            if tail.tab_id == write.tab_id
                && tail.generation == write.generation
                && tail.control_epoch == write.control_epoch
                && tail.user_initiated == write.user_initiated
                && tail.settled_at == write.settled_at
                && tail.data.len() + write.data.len() <= MAX_WRITE_BYTES =>
        {
            tail.data.push_str(&write.data);
        }
        _ => writes.push_back(write),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellfox_protocol::{Generation, TabId};

    fn write(tab_id: TabId, generation: Generation, data: &str) -> WriteTerminal {
        WriteTerminal {
            tab_id,
            generation,
            control_epoch: 1,
            data: data.into(),
            user_initiated: Some(true),
            settled_at: None,
        }
    }

    #[test]
    fn typing_joins_waiting_writes_in_order_and_pauses_at_the_bound() {
        let (tab, generation) = (TabId::new(), Generation::new());
        let mut writes = VecDeque::new();
        let text: String = "typed fast; ".repeat(10_000);
        for c in text.chars() {
            push(&mut writes, write(tab, generation, &c.to_string())).unwrap();
        }
        assert!(writes.len() <= text.len() / MAX_WRITE_BYTES + 1);
        assert!(
            writes
                .iter()
                .all(|write| write.data.len() <= MAX_WRITE_BYTES)
        );
        let joined: String = writes.iter().map(|write| write.data.as_str()).collect();
        assert_eq!(joined, text);
        // Another generation's input never joins this one's.
        push(&mut writes, write(tab, Generation::new(), "x")).unwrap();
        assert_eq!(writes.back().unwrap().data, "x");
        let chunk = "y".repeat(MAX_WRITE_BYTES);
        let error = loop {
            if let Err(error) = push(&mut writes, write(tab, generation, &chunk)) {
                break error;
            }
        };
        assert_eq!(error.to_string(), crate::terminal::runtime::PAUSED);
        let pending: usize = writes.iter().map(|write| write.data.len()).sum();
        assert!(pending <= MAX_PENDING_BYTES);
    }
}
