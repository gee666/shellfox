use super::{
    Pending,
    physical::{MAX_HELD, Physical},
};
use shellfox_ghostty_vt::{MouseAction, key::KeyEvent};
use shellfox_runtime::{Command, MAX_WHEEL_BATCH, Request, RequestId, SubmitError};
use shellfox_service::native::NativeInputContext;
use std::collections::{HashMap, VecDeque};

mod batch;
pub(crate) use batch::PAUSED;
use batch::{MAX_PENDING_BYTES, cost};

/// Queued wheel entries. Each carries up to u16::MAX coalesced steps.
const MAX_WHEEL_ENTRIES: usize = 256;
const MAX_IN_FLIGHT: usize = 64;
const CONTROL_CAPACITY: usize = 16;

struct Queued {
    command: Command,
    pending: Pending,
    repeats: u16,
    input: Option<NativeInputContext>,
    cost: usize,
}
#[derive(Default)]
pub(crate) struct Outbox {
    queue: VecDeque<Queued>,
    physical: Physical,
    wheel_entries: usize,
    controls: usize,
    /// Approximate memory held by queued commands; see batch::cost.
    bytes: usize,
    paused: bool,
}
fn control(command: &Command) -> bool {
    matches!(
        command,
        Command::Claim { .. }
            | Command::Replay { .. }
            | Command::RecoverAnsi { .. }
            | Command::Recover { .. }
            | Command::ObserverRecover { .. }
            | Command::Ack { .. }
            | Command::Screen { .. }
            | Command::Close { .. }
            | Command::RetryClose
    )
}
fn wheel(command: &Command) -> bool {
    matches!(command, Command::Mouse { event, .. } if event.action == MouseAction::Press
        && event.button.is_some_and(|button| super::physical::button_bit(button) == 0))
}
fn same_mouse(first: &Command, second: &Command) -> bool {
    matches!((first, second), (Command::Mouse {lease: a, event: x, geometry: g}, Command::Mouse {lease: b, event: y, geometry: h})
        if a == b && g == h && x.action == y.action && x.button == y.button && x.modifiers == y.modifiers
        && (x.action == MouseAction::Motion || x.x == y.x && x.y == y.y))
}
impl Outbox {
    pub fn is_drained(&self) -> bool {
        self.queue.is_empty() && !self.has_held()
    }
    pub fn has_held(&self) -> bool {
        self.physical.len() != 0
    }
    pub fn buttons(&self) -> u8 {
        self.physical.buttons()
    }
    pub fn key_release(&self, key: super::physical::KeyIdentity) -> Option<KeyEvent> {
        self.physical.key_release(key)
    }
    #[cfg(test)]
    pub fn cancel(&mut self, pointer_only: bool) -> Result<(), &'static str> {
        self.cancel_with_context(pointer_only, None)
    }
    pub fn cancel_with_context(
        &mut self,
        pointer_only: bool,
        input: Option<NativeInputContext>,
    ) -> Result<(), &'static str> {
        for command in self
            .physical
            .releases()
            .into_iter()
            .filter(|command| !pointer_only || matches!(command, Command::Mouse { .. }))
        {
            self.enqueue_with_input(command, Pending::Applied, input.clone())?;
        }
        Ok(())
    }

    pub fn stop_input(&mut self, cleanup_confirmed: bool) {
        self.queue.retain(|work| {
            !matches!(
                work.command,
                Command::Mouse { .. }
                    | Command::Key { .. }
                    | Command::Keys { .. }
                    | Command::Paste { .. }
                    | Command::RawInput { .. }
            )
        });
        self.wheel_entries = 0;
        self.bytes = self.queue.iter().map(|work| work.cost).sum();
        self.paused = false;
        if cleanup_confirmed {
            self.physical = Physical::default();
        }
    }

    #[cfg(test)]
    pub fn enqueue(&mut self, command: Command, pending: Pending) -> Result<(), &'static str> {
        self.enqueue_with_input(command, pending, None)
    }
    pub fn enqueue_with_input(
        &mut self,
        mut command: Command,
        pending: Pending,
        input: Option<NativeInputContext>,
    ) -> Result<(), &'static str> {
        let bytes = match &command {
            Command::Paste { text, .. } => text.len(),
            Command::RawInput { bytes, .. } => bytes.len(),
            Command::Key { event, .. } => event.text.len(),
            Command::Keys { events, .. } => events.iter().map(|event| event.text.len()).sum(),
            _ => 0,
        };
        if bytes > shellfox_runtime::MAX_INPUT_BYTES {
            return Err("Terminal input exceeds 64 KiB");
        }
        if self.physical.unmatched(&command) {
            return Ok(());
        }
        let is_wheel = wheel(&command);
        if matches!(pending, Pending::Applied)
            && let Some(tail) = self.queue.back_mut()
            && matches!(tail.pending, Pending::Applied)
            && same_mouse(&tail.command, &command)
            && tail.input == input
        {
            if is_wheel && tail.repeats < u16::MAX {
                tail.repeats += 1;
                return Ok(());
            }
            if matches!(command, Command::Mouse { event, .. } if event.action == MouseAction::Motion)
            {
                self.physical.accept(&mut command);
                tail.command = command;
                return Ok(());
            }
        }
        // Only a spin that keeps changing position or modifiers gets here. A
        // dropped wheel step loses a little scrolling; an error toast is worse.
        if is_wheel && self.wheel_entries >= MAX_WHEEL_ENTRIES {
            return Ok(());
        }
        let change = self.physical.change(&command);
        let held = self
            .physical
            .len()
            .checked_add_signed(isize::from(change))
            .ok_or("Invalid physical input state")?;
        let is_control = control(&command);
        if held > MAX_HELD || is_control && self.controls >= CONTROL_CAPACITY {
            return Err("Terminal busy: command was not accepted");
        }
        let cost = cost(&command);
        // Releases of held keys and buttons are always queued, so a pause
        // never leaves a key stuck. At most MAX_HELD of them exist.
        if !is_control && change >= 0 {
            if self.paused && self.bytes > MAX_PENDING_BYTES / 2 {
                return Err(PAUSED);
            }
            self.paused = self.bytes + cost > MAX_PENDING_BYTES;
            if self.paused {
                return Err(PAUSED);
            }
        }
        self.physical.accept(&mut command);
        self.bytes += cost;
        let Err(command) = batch::join(self.queue.back_mut(), command, &pending, &input) else {
            return Ok(());
        };
        self.queue.push_back(Queued {
            command,
            pending,
            repeats: 1,
            input,
            cost,
        });
        self.wheel_entries += usize::from(is_wheel);
        self.controls += usize::from(is_control);
        Ok(())
    }

    #[cfg(test)]
    pub fn flush(
        &mut self,
        next: &mut u64,
        pending: &mut HashMap<u64, Pending>,
        mut send: impl FnMut(Request) -> Result<(), SubmitError>,
    ) -> Option<SubmitError> {
        self.flush_with_input(next, pending, |request, _| send(request))
    }
    pub fn flush_with_input(
        &mut self,
        next: &mut u64,
        pending: &mut HashMap<u64, Pending>,
        mut send: impl FnMut(Request, Option<&NativeInputContext>) -> Result<(), SubmitError>,
    ) -> Option<SubmitError> {
        while pending.len() < MAX_IN_FLIGHT {
            let work = self.queue.pop_front()?;
            let Some(id) = next.checked_add(1) else {
                self.queue.push_front(work);
                return None;
            };
            let is_wheel = wheel(&work.command);
            let is_control = control(&work.command);
            // Coalesced wheel steps go out as one Wheel request per batch.
            let steps = work.repeats.min(MAX_WHEEL_BATCH);
            let (command, rest) = match work.command {
                Command::Mouse {
                    lease,
                    event,
                    geometry,
                } if steps > 1 => (
                    Command::Wheel {
                        lease,
                        event,
                        geometry,
                        steps,
                    },
                    (work.repeats > steps).then_some(Command::Mouse {
                        lease,
                        event,
                        geometry,
                    }),
                ),
                command => (command, None),
            };
            match send(
                Request {
                    id: RequestId(id),
                    command,
                },
                work.input.as_ref(),
            ) {
                Ok(()) => {
                    *next = id;
                    pending.insert(id, work.pending);
                    self.bytes -= work.cost;
                    self.controls -= usize::from(is_control);
                    if let Some(command) = rest {
                        self.queue.push_front(Queued {
                            command,
                            pending: Pending::Applied,
                            repeats: work.repeats - steps,
                            input: work.input,
                            cost: 0,
                        });
                    } else {
                        self.wheel_entries -= usize::from(is_wheel);
                    }
                }
                Err(SubmitError::Full(request)) => {
                    let command = match request.command {
                        Command::Wheel {
                            lease,
                            event,
                            geometry,
                            ..
                        } => Command::Mouse {
                            lease,
                            event,
                            geometry,
                        },
                        command => command,
                    };
                    self.queue.push_front(Queued {
                        command,
                        pending: work.pending,
                        repeats: work.repeats,
                        input: work.input,
                        cost: work.cost,
                    });
                    break;
                }
                Err(error) => {
                    self.bytes -= work.cost;
                    self.controls -= usize::from(is_control);
                    self.wheel_entries -= usize::from(is_wheel);
                    return Some(error);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod tests;
