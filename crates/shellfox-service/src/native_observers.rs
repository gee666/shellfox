use super::{
    cache::{Grant, View, ViewKey},
    *,
};
use anyhow::{ensure, Context, Result};
use shellfox_runtime::{AnsiRecoveryCapability, Command, ObserverLease};

fn key(controller: u64, tab: TabId) -> ViewKey {
    ViewKey {
        controller,
        tab,
        observer: true,
    }
}
impl crate::Service {
    pub(crate) fn has_native_observer_grant(
        &self,
        identity: shellfox_runtime::Identity,
        lease: ObserverLease,
    ) -> bool {
        self.terminals.get(&identity.tab_id).is_some_and(|terminal|terminal.runtime.identity()==identity) && self.native.views.get(&key(lease.viewer,identity.tab_id)).is_some_and(|view|matches!(&view.grant,Grant::Observer(current) if current.viewer==lease.viewer&&current.epoch==lease.epoch&&current.generation==identity.generation))
    }
    pub(super) fn native_observer(
        &self,
        tab: TabId,
        controller: u64,
        attachment: &NativeObserverAttachment,
    ) -> Result<ObserverLease> {
        ensure!(
            attachment.viewer == controller,
            "Observer attachment belongs to another client."
        );
        let terminal = self
            .terminals
            .get(&tab)
            .context("Observed terminal is not running.")?;
        ensure!(
            terminal.runtime.identity().generation == attachment.generation,
            "Observed terminal generation is stale."
        );
        let view = self
            .native
            .views
            .get(&key(controller, tab))
            .context("Observer view is detached.")?;
        ensure!(
            matches!(&view.grant, Grant::Observer(current) if current == attachment),
            "Observer attachment is stale."
        );
        Ok(ObserverLease {
            viewer: attachment.viewer,
            epoch: attachment.epoch,
        })
    }
    pub(super) fn native_observe(&mut self, tab: TabId, controller: u64) -> Result<NativeResponse> {
        let view_key = key(controller, tab);
        self.native.admit(view_key)?;
        let terminal = self
            .terminals
            .get(&tab)
            .context("Read-only views cannot create a new terminal.")?;
        let generation = terminal.runtime.identity().generation;
        let offer = self.observer_grant(tab, controller)?;
        let observer = offer.lease();
        let attachment = NativeObserverAttachment {
            viewer: observer.viewer,
            epoch: observer.epoch,
            generation,
        };
        self.native.views.insert(
            view_key,
            View::new(
                Grant::Observer(attachment.clone()),
                0,
                self.terminals[&tab].runtime.phase(),
            ),
        );
        if let Err(error) =
            self.adopt_observer_grant(offer, crate::grant_requests::GrantHolder::NativeObserver)
        {
            self.native.views.remove(&view_key); // Central offer retains retirement; no second Unobserve.
            return Err(error);
        }
        self.native_recover_observer(tab, controller, &attachment)?;
        Ok(NativeResponse::Observed(attachment))
    }
    pub(super) fn native_recover_observer(
        &mut self,
        tab: TabId,
        controller: u64,
        attachment: &NativeObserverAttachment,
    ) -> Result<NativeResponse> {
        let observer = self.native_observer(tab, controller, attachment)?;
        let Response::Ansi { last_sequence, .. } = self.call(
            tab,
            Command::ObserverRecover {
                observer,
                capability: AnsiRecoveryCapability::VisibleScreen,
            },
        )?
        else {
            anyhow::bail!("Runtime returned an invalid observer recovery barrier.");
        };
        self.native.views.insert(
            key(controller, tab),
            View::new(
                Grant::Observer(attachment.clone()),
                last_sequence,
                self.terminals[&tab].runtime.phase(),
            ),
        );
        Ok(NativeResponse::Applied)
    }
    pub(super) fn native_observer_frame(
        &mut self,
        tab: TabId,
        controller: u64,
        attachment: &NativeObserverAttachment,
    ) -> Result<NativeResponse> {
        self.native_presentation(
            tab,
            controller,
            NativeViewAttachment::Observer(attachment.clone()),
            NativePresentation::Frame { focused: true },
        )
    }
    pub(super) fn native_accept_observer(
        &mut self,
        tab: TabId,
        controller: u64,
        attachment: &NativeObserverAttachment,
        frame_id: u64,
    ) -> Result<NativeResponse> {
        let observer = self.native_observer(tab, controller, attachment)?;
        let view_key = key(controller, tab);
        let sequence = self
            .native
            .views
            .get_mut(&view_key)
            .context("Observer view detached.")?
            .accepted_sequence(frame_id)?;
        if self.native.views[&view_key]
            .acked
            .is_none_or(|last| sequence > last)
        {
            self.call(tab, Command::ObserverAck { observer, sequence })?;
            let view = self
                .native
                .views
                .get_mut(&view_key)
                .context("Observer view detached.")?;
            view.acked = Some(sequence);
            view.barrier = None;
        }
        Ok(NativeResponse::Accepted { sequence })
    }
    pub(super) fn native_unobserve(
        &mut self,
        tab: TabId,
        controller: u64,
        attachment: &NativeObserverAttachment,
    ) -> Result<()> {
        let observer = self.native_observer(tab, controller, attachment)?;
        self.call(tab, Command::Unobserve { observer })?;
        self.native.remove(key(controller, tab));
        Ok(())
    }
    pub(super) fn native_detach_all(&mut self, controller: u64) -> Result<()> {
        let observers: Vec<_> = self
            .native
            .views
            .iter()
            .filter_map(|(key, view)| {
                if key.controller != controller {
                    return None;
                }
                match &view.grant {
                    Grant::Observer(a) => Some((key.tab, a.clone())),
                    _ => None,
                }
            })
            .collect();
        for (tab, attachment) in observers {
            self.native_unobserve(tab, controller, &attachment)?;
        }
        self.detach_controller(controller);
        self.native
            .views
            .retain(|key, _| key.controller != controller);
        self.native.prune();
        Ok(())
    }
}
