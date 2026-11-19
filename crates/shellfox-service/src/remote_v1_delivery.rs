use super::*;
use crate::terminals::{Attachment, TerminalOutput};
use shellfox_core::TabId;
use std::collections::VecDeque;

pub(super) struct Delivery {
    pub lease: Attachment,
    utf8: Utf8Stream,
    raw_seen: u64,
    wire_sent: u64,
    wire_acked: Option<u64>,
    pending: VecDeque<(u64, u64)>,
    faulted: bool,
    exited: bool,
}

#[derive(Default)]
struct Utf8Stream(Vec<u8>);
impl Utf8Stream {
    fn decode(&mut self, bytes: &[u8]) -> String {
        self.0.extend_from_slice(bytes);
        let mut text = String::new();
        let mut data = self.0.as_slice();
        loop {
            match std::str::from_utf8(data) {
                Ok(s) => {
                    text.push_str(s);
                    data = &[];
                    break;
                }
                Err(err) => {
                    let (valid, rest) = data.split_at(err.valid_up_to());
                    text.push_str(&String::from_utf8_lossy(valid));
                    let Some(invalid) = err.error_len() else {
                        data = rest;
                        break;
                    };
                    text.push('\u{fffd}');
                    data = &rest[invalid..];
                }
            }
        }
        self.0 = data.to_vec();
        text
    }
}

impl Delivery {
    pub fn snapshot(
        lease: Attachment,
        tab_id: TabId,
        data: &[u8],
        raw_cut: u64,
    ) -> (Self, Vec<wire::TerminalData>) {
        let mut utf8 = Utf8Stream::default();
        let text = utf8.decode(data);
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < text.len() {
            let end = text.floor_char_boundary((start + 16384).min(text.len()));
            chunks.push(wire::TerminalData {
                kind: wire::DataTag::Data,
                tab_id,
                generation: lease.generation,
                sequence: chunks.len() as u64 + 1,
                data: text[start..end].to_owned(),
            });
            start = end;
        }
        let wire_sent = chunks.len() as u64;
        (
            Self {
                lease,
                utf8,
                raw_seen: raw_cut,
                wire_sent,
                wire_acked: None,
                pending: VecDeque::from([(wire_sent, raw_cut)]),
                faulted: false,
                exited: false,
            },
            chunks,
        )
    }

    pub fn acknowledge(
        &mut self,
        service: &mut Service,
        tab: TabId,
        sequence: u64,
    ) -> Result<(), AppError> {
        if self.wire_acked == Some(sequence) {
            return Ok(());
        }
        let raw = self
            .pending
            .iter()
            .find(|&(wire, _)| *wire == sequence)
            .map(|&(_, raw)| raw)
            .ok_or_else(|| {
                failure(
                    ErrorCode::Validation,
                    "Acknowledgement is outside the delivered sequence range.",
                )
            })?;
        // A final incomplete codepoint is held in the decoder, not lost. Release
        // its raw credit too once every emitted event has been consumed.
        let raw = if sequence == self.wire_sent {
            self.raw_seen
        } else {
            raw
        };
        service
            .acknowledge(tab, &self.lease, raw)
            .map_err(|_| super::v1_terminal::lost())?;
        self.wire_acked = Some(sequence);
        self.pending.retain(|&(wire, _)| wire > sequence);
        Ok(())
    }

    pub fn poll(
        &mut self,
        service: &mut Service,
        tab_id: TabId,
        quota: usize,
        events: &mut Vec<wire::HostEvent>,
    ) {
        if self.faulted || self.exited {
            return;
        }
        let generation = self.lease.generation;
        let control_epoch = self.lease.epoch;
        let event = |event| wire::HostEvent::Terminal { event };
        if service.lease(tab_id, &self.lease).is_err() {
            self.faulted = true;
            events.push(event(wire::TerminalEvent::Control {
                tab_id,
                generation,
                control_epoch,
                control: wire::Control::Blocked,
            }));
            return;
        }
        let Ok((output, replay_lost, exited)) =
            service.remote_v1_output(tab_id, &self.lease, self.raw_seen)
        else {
            return;
        };
        if replay_lost {
            self.faulted = true;
            events.push(event(wire::TerminalEvent::Error {
                tab_id,
                generation,
                control_epoch,
                error: AppError {
                    code: ErrorCode::NativeUnavailable,
                    message:
                        "Terminal output exceeded the replay window. Reattach for a fresh snapshot."
                            .into(),
                    retryable: true,
                },
            }));
            return;
        }
        let initial = self.raw_seen;
        let limit = events.len() + quota;
        for TerminalOutput { sequence, bytes } in output {
            if events.len() >= limit {
                break;
            }
            let data = self.utf8.decode(&bytes);
            self.raw_seen = sequence;
            if data.is_empty() {
                continue;
            }
            self.wire_sent += 1;
            self.pending.push_back((self.wire_sent, sequence));
            events.push(event(wire::TerminalEvent::Data {
                tab_id,
                generation,
                control_epoch,
                sequence: self.wire_sent,
                data,
            }));
        }
        if self.raw_seen != initial && self.pending.is_empty() {
            let _ = service.acknowledge(tab_id, &self.lease, self.raw_seen);
        }
        if exited.is_some_and(|last| last == self.raw_seen) {
            if !self.utf8.0.is_empty() {
                self.utf8.0.clear();
                self.wire_sent += 1;
                self.pending.push_back((self.wire_sent, self.raw_seen));
                events.push(event(wire::TerminalEvent::Data {
                    tab_id,
                    generation,
                    control_epoch,
                    sequence: self.wire_sent,
                    data: "\u{fffd}".into(),
                }));
            }
            self.exited = true;
            events.push(event(wire::TerminalEvent::Exit {
                tab_id,
                generation,
                control_epoch,
                last_sequence: self.wire_sent,
                exit_code: None,
                signal: None,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Utf8Stream;

    #[test]
    fn decodes_utf8_split_at_snapshot_and_output_boundaries() {
        let mut decoder = Utf8Stream::default();
        assert_eq!(decoder.decode(b"start\xf0"), "start");
        assert_eq!(decoder.decode(b"\x9f\x92"), "");
        assert_eq!(decoder.decode(b"\xa9\xff!"), "💩\u{fffd}!");
    }
}
