use super::*;

impl DesktopLaunchRouter {
    /// Recheck actual ownership and the current desktop token immediately before
    /// UI effects. Queued messages surviving a dropped owner are not authority.
    pub fn claim(
        &mut self,
        delivery: &DesktopLaunchDelivery,
        host: &mut impl LaunchSessionResolver,
    ) -> Result<LaunchClaim, LaunchError> {
        self.check(delivery.client.owner, host)?;
        self.check_client(delivery.client)?;
        delivery.validate()?;
        let entry = self
            .entries
            .get(&delivery.intent.request_id)
            .ok_or(LaunchError::ReceiptUnavailable)?;
        if entry.intent != delivery.intent || entry.target.as_ref() != Some(&delivery.target) {
            return Err(LaunchError::Conflict);
        }
        if entry.client != Some(delivery.client) {
            return Err(LaunchError::StaleClient);
        }
        let resolved = match host.resolve(&delivery.intent, LaunchRequestKind::Known) {
            Ok(target) => target,
            Err(error) => {
                self.check(delivery.client.owner, host)?;
                self.receipts_changed();
                self.entries
                    .get_mut(&delivery.intent.request_id)
                    .ok_or(LaunchError::ReceiptUnavailable)?
                    .state = LaunchState::Failed(error);
                return Err(error);
            }
        };
        self.check(delivery.client.owner, host)?;
        if resolved != delivery.target {
            return Err(LaunchError::Conflict);
        }
        self.receipts_changed();
        let entry = self
            .entries
            .get_mut(&delivery.intent.request_id)
            .ok_or(LaunchError::ReceiptUnavailable)?;
        match entry.state {
            LaunchState::Dispatched | LaunchState::Applying => {
                // The registration owns one non-cloneable receiver/coordinator.
                // Repeating an interrupted claim does not repeat UI effects.
                entry.state = LaunchState::Applying;
                Ok(LaunchClaim::Apply)
            }
            LaunchState::Handled => Ok(LaunchClaim::AlreadyHandled),
            LaunchState::Uncertain => Err(LaunchError::DeliveryUncertain),
            LaunchState::Failed(error) => Err(error),
            LaunchState::Queued => Err(LaunchError::Invalid),
        }
    }
    pub fn complete(
        &mut self,
        delivery: &DesktopLaunchDelivery,
        handling: LaunchHandling,
        authority: &impl LaunchAuthority,
    ) -> Result<LaunchReceipt, LaunchError> {
        self.check(delivery.client.owner, authority)?;
        self.check_client(delivery.client)?;
        delivery.validate()?;
        let entry = self
            .entries
            .get_mut(&delivery.intent.request_id)
            .ok_or(LaunchError::ReceiptUnavailable)?;
        if entry.intent != delivery.intent || entry.target.as_ref() != Some(&delivery.target) {
            return Err(LaunchError::Conflict);
        }
        if entry.client != Some(delivery.client) {
            return Err(LaunchError::StaleClient);
        }
        let state = match handling {
            LaunchHandling::Handled => LaunchState::Handled,
            LaunchHandling::Failed(LaunchError::DeliveryUncertain) => LaunchState::Uncertain,
            LaunchHandling::Failed(error) => LaunchState::Failed(error),
        };
        if entry.state != LaunchState::Applying && entry.state != state {
            return Err(LaunchError::Conflict);
        }
        entry.state = state;
        self.receipts_changed();
        Ok(LaunchReceipt {
            request_id: delivery.intent.request_id,
            state,
        })
    }
}
