//! Terminals a view shows while another view controls them. As 0.9 promoted a
//! waiting viewer when the owner left, each view hears once that control is
//! free (an `owned` control event) and reattaches to take it.
use super::*;
use shellfox_core::{Generation, TabId};

/// The most terminals one view waits for; as many as it may attach.
const MAX_WAITING: usize = 16;

#[derive(Default)]
pub(super) struct Waiting(HashMap<TabId, Generation>);
impl Waiting {
    pub(super) fn wait(&mut self, tab: TabId, generation: Generation) {
        if self.0.len() < MAX_WAITING || self.0.contains_key(&tab) {
            self.0.insert(tab, generation);
        }
    }
    pub(super) fn stop(&mut self, tab: TabId) {
        self.0.remove(&tab);
    }
    pub(super) fn has_events(&self, service: &Service) -> bool {
        self.0
            .iter()
            .any(|(&tab, &generation)| service.free_control_epoch(tab, generation).is_some())
    }
    /// Tells the view about each terminal whose control is free, once. A
    /// terminal that restarted or closed is dropped; its own events tell.
    pub(super) fn poll(&mut self, service: &Service, events: &mut Vec<wire::HostEvent>) {
        self.0.retain(|&tab_id, &mut generation| {
            let running = service
                .terminals
                .get(&tab_id)
                .is_some_and(|terminal| terminal.runtime.identity().generation == generation);
            let Some(control_epoch) = service.free_control_epoch(tab_id, generation) else {
                return running;
            };
            events.push(wire::HostEvent::Terminal {
                event: wire::TerminalEvent::Control {
                    tab_id,
                    generation,
                    control_epoch,
                    control: wire::Control::Owned,
                },
            });
            false
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_waits_for_at_most_sixteen_terminals() {
        let mut waiting = Waiting::default();
        for _ in 0..20 {
            waiting.wait(TabId::new(), Generation::new());
        }
        assert_eq!(waiting.0.len(), MAX_WAITING);
        let tab = *waiting.0.keys().next().unwrap();
        waiting.wait(tab, Generation::new());
        assert_eq!(waiting.0.len(), MAX_WAITING);
        waiting.stop(tab);
        assert_eq!(waiting.0.len(), MAX_WAITING - 1);
    }
}
