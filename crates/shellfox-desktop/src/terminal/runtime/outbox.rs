use super::{
    Pending,
    physical::{MAX_HELD, Physical},
};
use shellfox_ghostty_vt::{MouseAction, key::KeyEvent};
use shellfox_runtime::{Command, Request, RequestId, SubmitError};
use shellfox_service::native::NativeInputContext;
use std::collections::{HashMap, VecDeque};

const NORMAL_CAPACITY: usize = 64;
const CAPACITY: usize = NORMAL_CAPACITY + MAX_HELD;
const MAX_WHEEL_STEPS: usize = 256;
const MAX_IN_FLIGHT: usize = 64;
const CONTROL_CAPACITY: usize = 16;

struct Queued {
    command: Command,
    pending: Pending,
    repeats: u16,
    input: Option<NativeInputContext>,
}
#[derive(Default)]
pub(crate) struct Outbox {
    queue: VecDeque<Queued>,
    physical: Physical,
    wheel_steps: usize,
    controls: usize,
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
                    | Command::Paste { .. }
                    | Command::RawInput { .. }
            )
        });
        self.wheel_steps = 0;
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
            _ => 0,
        };
        if bytes > shellfox_runtime::MAX_INPUT_BYTES {
            return Err("Terminal input exceeds 64 KiB");
        }
        if self.physical.unmatched(&command) {
            return Ok(());
        }
        let is_wheel = wheel(&command);
        if is_wheel && self.wheel_steps >= MAX_WHEEL_STEPS {
            return Err("Terminal wheel backlog is full");
        }
        if matches!(pending, Pending::Applied)
            && let Some(tail) = self.queue.back_mut()
            && matches!(tail.pending, Pending::Applied)
            && same_mouse(&tail.command, &command)
            && tail.input == input
        {
            if is_wheel {
                tail.repeats += 1;
                self.wheel_steps += 1;
                return Ok(());
            }
            if matches!(command, Command::Mouse { event, .. } if event.action == MouseAction::Motion)
            {
                self.physical.accept(&mut command);
                tail.command = command;
                return Ok(());
            }
        }
        let change = self.physical.change(&command);
        let held = self
            .physical
            .len()
            .checked_add_signed(isize::from(change))
            .ok_or("Invalid physical input state")?;
        let is_control = control(&command);
        let ordinary = self.queue.len() - self.controls;
        if held > MAX_HELD
            || is_control && self.controls >= CONTROL_CAPACITY
            || !is_control
                && (ordinary + 1 + held > CAPACITY || change == 0 && ordinary >= NORMAL_CAPACITY)
        {
            return Err("Terminal busy: command was not accepted");
        }
        // Each accepted press reserves one future release slot. Releases cannot
        // overtake their press or keys, even when all ordinary slots are full.
        self.physical.accept(&mut command);
        self.queue.push_back(Queued {
            command,
            pending,
            repeats: 1,
            input,
        });
        self.wheel_steps += usize::from(is_wheel);
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
            let repeated = match &work.command {
                Command::Mouse {
                    lease,
                    event,
                    geometry,
                } if work.repeats > 1 => Some(Command::Mouse {
                    lease: *lease,
                    event: *event,
                    geometry: *geometry,
                }),
                _ => None,
            };
            let is_wheel = wheel(&work.command);
            let is_control = control(&work.command);
            match send(
                Request {
                    id: RequestId(id),
                    command: work.command,
                },
                work.input.as_ref(),
            ) {
                Ok(()) => {
                    *next = id;
                    pending.insert(id, work.pending);
                    self.wheel_steps -= usize::from(is_wheel);
                    self.controls -= usize::from(is_control);
                    if let Some(command) = repeated {
                        self.queue.push_front(Queued {
                            command,
                            pending: Pending::Applied,
                            repeats: work.repeats - 1,
                            input: work.input,
                        });
                    }
                }
                Err(SubmitError::Full(request)) => {
                    self.queue.push_front(Queued {
                        command: request.command,
                        pending: work.pending,
                        repeats: work.repeats,
                        input: work.input,
                    });
                    break;
                }
                Err(error) => {
                    self.controls -= usize::from(is_control);
                    self.wheel_steps -= if is_wheel {
                        usize::from(work.repeats)
                    } else {
                        0
                    };
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
