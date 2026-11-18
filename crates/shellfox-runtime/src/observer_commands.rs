use crate::{owner::Owner, *};

impl Owner {
    pub(crate) fn observer_command(&mut self, command: Command) -> Result<Response, RuntimeError> {
        match command {
            Command::Observe { viewer, after } => {
                let observer = self.observers.observe(viewer, after, &self.delivery)?;
                self.presentations.observer_changed(observer);
                self.layouts.observer_changed(observer);
                Ok(Response::Observed(observer))
            }
            Command::ObserverReplay { observer, after } => {
                self.observers.replay(observer, after, &self.delivery)?;
                Ok(Response::Applied)
            }
            Command::ObserverAck { observer, sequence } => {
                if !self.layouts.data_ack_allowed(observer) {
                    return Err(RuntimeError::validation(
                        "Stamped recovery requires its exact validated recovery ACK.",
                    ));
                }
                self.observers.acknowledge(observer, sequence)?;
                Ok(Response::Applied)
            }
            Command::ObserverRecover {
                observer,
                capability,
            } => {
                self.observers.check(observer)?;
                let bytes = self.recovery_bytes(capability)?;
                self.layouts.remove(observer);
                let last_sequence = self.observers.recover(observer, &self.delivery)?;
                Ok(Response::Ansi {
                    capability,
                    bytes,
                    last_sequence,
                })
            }
            Command::ObserverRecoverStamped {
                observer,
                capability,
            } => self.stamped_recovery(observer, capability),
            #[cfg(feature = "experimental-shellfox-native")]
            Command::ObserverRecoverNative0Stamped { observer } => {
                self.observers.check(observer)?;
                let (snapshot, limits) = self.native0_snapshot()?;
                let cut = self
                    .layouts
                    .native0_recovery(observer, self.delivery.current_sequence())?;
                self.observers.recover(observer, &self.delivery)?;
                Ok(Response::Native0Recovery(Box::new(
                    StampedNative0Recovery {
                        snapshot,
                        limits,
                        cut,
                    },
                )))
            }
            Command::ObserverLayoutAck {
                observer,
                recovery_id,
                last_delivery_sequence,
                layout_revision,
            } => self.recovery_ack(
                observer,
                recovery_id,
                last_delivery_sequence,
                layout_revision,
            ),
            Command::Unobserve { observer } => {
                self.observers.unobserve(observer)?;
                self.presentations.unobserve(observer);
                self.layouts.remove(observer);
                Ok(Response::Applied)
            }
            _ => unreachable!("only observer commands enter this dispatch"),
        }
    }
}
