use super::*;

pub(super) struct AttachedRegistration {
    id: uuid::Uuid,
    epoch: uuid::Uuid,
    registration: DesktopRegistration,
}
impl PrivateOwner {
    pub(super) fn admits_attached_provider(&self, token: DesktopClientToken) -> bool {
        self.attached_desktop.as_ref().is_some_and(|attached| {
            attached.registration.token == token
                && self
                    .lifetimes
                    .get(&attached.id)
                    .map(|lifetime| lifetime.epoch)
                    == Some(attached.epoch)
        })
    }

    pub(super) fn attached_launch(
        &mut self,
        service: &mut Service,
        id: uuid::Uuid,
        epoch: Option<uuid::Uuid>,
        request: DesktopLaunchRequest,
    ) -> Result<DesktopLaunchReply> {
        let epoch =
            epoch.context("Attached desktop requires its original local client lifetime.")?;
        if let DesktopLaunchRequest::RegisterAttached { owner } = request {
            service.assert_accepting()?;
            let registration = self.launches.register(owner, service)?;
            let token = registration.token;
            self.attached_desktop = Some(AttachedRegistration {
                id,
                epoch,
                registration,
            });
            return Ok(DesktopLaunchReply::Registered(token));
        }
        let attached = self
            .attached_desktop
            .as_mut()
            .filter(|entry| entry.id == id && entry.epoch == epoch)
            .ok_or(LaunchError::StaleClient)?;
        let token = attached.registration.token;
        match request {
            DesktopLaunchRequest::PollAttached { token: requested } if requested == token => Ok(
                DesktopLaunchReply::Delivery(attached.registration.poll_delivery()?),
            ),
            DesktopLaunchRequest::ClaimAttached { delivery } if delivery.client == token => {
                service.assert_accepting()?;
                Ok(DesktopLaunchReply::Claim(self.launches.claim(
                    &delivery,
                    &mut ServiceLaunchResolver::new(service, &ExistingLocalDirectory),
                )?))
            }
            DesktopLaunchRequest::CompleteAttached { delivery, handling }
                if delivery.client == token =>
            {
                Ok(DesktopLaunchReply::Receipt(
                    self.launches.complete(&delivery, handling, service)?,
                ))
            }
            DesktopLaunchRequest::UnregisterAttached { token: requested } if requested == token => {
                self.launches.unregister(token, service)?;
                self.attached_desktop = None;
                Ok(DesktopLaunchReply::Unregistered {})
            }
            _ => Err(LaunchError::Invalid.into()),
        }
    }

    pub(super) fn forget_attached_desktop(&mut self, service: &Service, id: uuid::Uuid) {
        if self
            .attached_desktop
            .as_ref()
            .is_some_and(|entry| entry.id == id)
        {
            if let Some(entry) = self.attached_desktop.take() {
                let _ = self.launches.unregister(entry.registration.token, service);
            }
        }
    }
}
