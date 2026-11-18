use crate::{observers::ObserverEvent, owner::Owner, *};
use std::sync::mpsc::{SyncSender, TrySendError};

impl Owner {
    pub(crate) fn flush(&mut self, events: &SyncSender<Event>) -> bool {
        while let Some(event) = self.outgoing.pop_front() {
            match events.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(event)) => {
                    self.outgoing.push_front(event);
                    return false;
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.shared.abandoned.store(true, Ordering::Release);
                    return false;
                }
            }
        }
        true
    }
    pub(crate) fn emit_events(&mut self, events: &SyncSender<Event>) {
        // Replies and recovery barriers are always queued before dependent data.
        if !self.flush(events) {
            return;
        }
        let observers_first = self.observers_first;
        self.observers_first = !observers_first;
        if observers_first {
            self.emit_observers(events);
        }
        // No staged controller data may consume the next observer-first round.
        // Both paths commit credit only when the same bounded channel accepts.
        for _ in 0..2 {
            if !self.controller_delivery_allowed(events) {
                break;
            }
            let Some(lease) = self.delivery.lease() else {
                break;
            };
            let shared = &self.shared;
            if !self.delivery.pump_live(|chunk| {
                match events.try_send(Event::Output { lease, chunk }) {
                    Ok(()) => true,
                    Err(TrySendError::Full(_)) => false,
                    Err(TrySendError::Disconnected(_)) => {
                        shared.abandoned.store(true, Ordering::Release);
                        false
                    }
                }
            }) {
                break;
            }
        }
        if self.outgoing.len() < 2 && self.delivery.take_replay_lost() {
            if let Some(lease) = self.delivery.lease() {
                self.layouts.controller_lost(lease);
                self.outgoing.push_back(Event::ReplayLost {
                    lease,
                    last_sequence: self.delivery.current_sequence(),
                });
            }
        }
        if self.controller_delivery_allowed(events)
            && self.phase == Phase::Closed
            && !self.exit_sent
            && self.outgoing.len() < 2
            && self.delivery.last_sent() >= self.delivery.current_sequence()
        {
            if let Some(lease) = self.delivery.lease() {
                self.outgoing.push_back(Event::Exited {
                    lease,
                    last_sequence: self.delivery.current_sequence(),
                });
                self.exit_sent = true;
            }
        }
        if !self.flush(events) {
            return;
        }
        if !observers_first {
            self.emit_observers(events);
        }
    }
    fn controller_delivery_allowed(&mut self, events: &SyncSender<Event>) -> bool {
        let Some(lease) = self.delivery.lease() else {
            return true;
        };
        let shared = &self.shared;
        self.layouts.controller_allows(
            lease,
            self.delivery.last_sent().saturating_add(1),
            self.delivery.current_sequence(),
            |notice| {
                let event = match notice {
                    crate::layout::LayoutNotice::Changed(layout) => {
                        Event::ControllerLayout { lease, layout }
                    }
                    crate::layout::LayoutNotice::Lost(last_sequence) => Event::ReplayLost {
                        lease,
                        last_sequence,
                    },
                };
                queued(events, shared, event)
            },
        )
    }
    fn emit_observers(&mut self, events: &SyncSender<Event>) {
        let current = self.delivery.current_sequence();
        let shared = &self.shared;
        // Notify an already-delivered geometry boundary even when no new bytes
        // arrive. This is not a grant; only a fresh full recovery can release it.
        for observer in self.layouts.observers() {
            if let Some(sent) = self.observers.sent_sequence(observer) {
                self.layouts
                    .allows(observer, sent.saturating_add(1), current, |notice| {
                        queued(events, shared, layout_event(observer, notice))
                    });
            }
        }
        let layouts = &mut self.layouts;
        self.observers
            .pump(&self.delivery, self.phase == Phase::Closed, |event| {
                let boundary = match &event {
                    ObserverEvent::Output { lease, chunk } => Some((*lease, chunk.sequence)),
                    ObserverEvent::Exited {
                        lease,
                        last_sequence,
                    } => Some((*lease, last_sequence.saturating_add(1))),
                    ObserverEvent::ReplayLost { .. } => None,
                };
                if let Some((observer, next)) = boundary {
                    if !layouts.allows(observer, next, current, |notice| {
                        queued(events, shared, layout_event(observer, notice))
                    }) {
                        return false;
                    }
                }
                let event = match event {
                    ObserverEvent::Output { lease, chunk } => Event::ObserverOutput {
                        observer: lease,
                        chunk,
                    },
                    ObserverEvent::ReplayLost {
                        lease,
                        last_sequence,
                    } => Event::ObserverReplayLost {
                        observer: lease,
                        last_sequence,
                    },
                    ObserverEvent::Exited {
                        lease,
                        last_sequence,
                    } => Event::ObserverExited {
                        observer: lease,
                        last_sequence,
                    },
                };
                queued(events, shared, event)
            });
    }
}
fn layout_event(observer: ObserverLease, notice: crate::layout::LayoutNotice) -> Event {
    match notice {
        crate::layout::LayoutNotice::Changed(layout) => Event::ObserverLayout { observer, layout },
        crate::layout::LayoutNotice::Lost(last_sequence) => Event::ObserverReplayLost {
            observer,
            last_sequence,
        },
    }
}
fn queued(events: &SyncSender<Event>, shared: &Shared, event: Event) -> bool {
    match events.try_send(event) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) => false,
        Err(TrySendError::Disconnected(_)) => {
            shared.abandoned.store(true, Ordering::Release);
            false
        }
    }
}
