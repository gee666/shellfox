use super::*;

impl DesktopLaunchRouter {
    /// Only the authenticated native/local desktop adapter registers. The public
    /// remote/mobile method table must never expose this receiver or capability.
    pub fn register(
        &mut self,
        owner: LaunchOwnerFence,
        authority: &impl LaunchAuthority,
    ) -> Result<DesktopRegistration, LaunchError> {
        self.check(owner, authority)?;
        if let Some(desktop) = &self.desktop {
            if !desktop.sender.is_closed() {
                return Err(LaunchError::Busy);
            }
            let token = desktop.token;
            self.retire_client(token);
        }
        let token = DesktopClientToken {
            owner,
            nonce: Uuid::new_v4(),
        };
        let (sender, receiver) = mpsc::channel(DESKTOP_CHANNEL_CAPACITY);
        self.desktop = Some(Registered { token, sender });
        Ok(DesktopRegistration { token, receiver })
    }
    pub fn unregister(
        &mut self,
        token: DesktopClientToken,
        authority: &impl LaunchAuthority,
    ) -> Result<(), LaunchError> {
        self.check(token.owner, authority)?;
        self.check_client(token)?;
        self.retire_client(token);
        Ok(())
    }
    fn retire_client(&mut self, token: DesktopClientToken) {
        self.receipts_changed();
        self.desktop = None;
        for entry in self.entries.values_mut() {
            if entry.client == Some(token)
                && matches!(entry.state, LaunchState::Dispatched | LaunchState::Applying)
            {
                // A native open/focus may already have happened. Never move an
                // ambiguous old delivery to a newly registered coordinator.
                entry.state = LaunchState::Uncertain;
            }
        }
    }
    pub(super) fn check_client(&self, token: DesktopClientToken) -> Result<(), LaunchError> {
        if self
            .desktop
            .as_ref()
            .is_some_and(|desktop| desktop.token == token && !desktop.sender.is_closed())
        {
            Ok(())
        } else {
            Err(LaunchError::StaleClient)
        }
    }
    pub fn poll(
        &mut self,
        registration: &mut DesktopRegistration,
        authority: &impl LaunchAuthority,
    ) -> Result<Option<DesktopLaunchDelivery>, LaunchError> {
        self.check(registration.token.owner, authority)?;
        self.check_client(registration.token)?;
        registration.poll_delivery()
    }
    pub fn route_info(
        &mut self,
        authority: &impl LaunchAuthority,
    ) -> Result<LaunchRouteInfo, LaunchError> {
        self.check(self.owner, authority)?;
        Ok(LaunchRouteInfo {
            owner: self.owner,
            desktop_registered: self
                .desktop
                .as_ref()
                .is_some_and(|desktop| !desktop.sender.is_closed()),
            build: super::own_build_stamp(),
        })
    }
    pub fn enqueue(
        &mut self,
        owner: LaunchOwnerFence,
        intent: DesktopLaunchIntent,
        authority: &impl LaunchAuthority,
    ) -> Result<LaunchReceipt, LaunchError> {
        self.enqueue_kind(owner, intent, LaunchRequestKind::Fresh, authority)
    }
    /// Recovery of an already known launch ID. With no in-memory receipt, only
    /// the durable creation mapping can resolve it; UI-only effects stay unknown.
    pub fn resume(
        &mut self,
        owner: LaunchOwnerFence,
        intent: DesktopLaunchIntent,
        authority: &impl LaunchAuthority,
    ) -> Result<LaunchReceipt, LaunchError> {
        self.enqueue_kind(owner, intent, LaunchRequestKind::Known, authority)
    }
    fn enqueue_kind(
        &mut self,
        owner: LaunchOwnerFence,
        intent: DesktopLaunchIntent,
        kind: LaunchRequestKind,
        authority: &impl LaunchAuthority,
    ) -> Result<LaunchReceipt, LaunchError> {
        self.check(owner, authority)?;
        intent.validate()?;
        if let Some(entry) = self.entries.get(&intent.request_id) {
            return if entry.intent == intent {
                Ok(LaunchReceipt {
                    request_id: intent.request_id,
                    state: entry.state,
                })
            } else {
                Err(LaunchError::Conflict)
            };
        }
        if kind == LaunchRequestKind::Known
            && !matches!(intent.action, LaunchAction::NewSession { .. })
        {
            return Err(LaunchError::DeliveryUncertain);
        }
        if self.pending.len() >= MAX_PENDING_LAUNCHES || self.entries.len() >= MAX_LAUNCH_RECEIPTS {
            return Err(LaunchError::Full);
        }
        let request_id = intent.request_id;
        self.entries.insert(
            request_id,
            Entry {
                intent,
                kind,
                target: None,
                client: None,
                state: LaunchState::Queued,
            },
        );
        self.receipts_changed();
        self.pending.push_back(request_id);
        Ok(LaunchReceipt {
            request_id,
            state: LaunchState::Queued,
        })
    }
    pub fn receipt(
        &mut self,
        owner: LaunchOwnerFence,
        request_id: SessionId,
        authority: &impl LaunchAuthority,
    ) -> Result<LaunchReceipt, LaunchError> {
        self.check(owner, authority)?;
        let entry = self
            .entries
            .get(&request_id)
            .ok_or(LaunchError::ReceiptUnavailable)?;
        Ok(LaunchReceipt {
            request_id,
            state: entry.state,
        })
    }
    /// Reserve channel capacity BEFORE metadata creation. A full/absent desktop
    /// cannot cause extra sessions, and dispatch never starts or attaches a PTY.
    pub fn dispatch_one(
        &mut self,
        owner: LaunchOwnerFence,
        host: &mut impl LaunchSessionResolver,
    ) -> Result<Option<LaunchReceipt>, LaunchError> {
        self.check(owner, host)?;
        let Some(request_id) = self.pending.front().copied() else {
            return Ok(None);
        };
        let desktop = self.desktop.as_ref().ok_or(LaunchError::NoDesktop)?;
        let token = desktop.token;
        let permit = desktop
            .sender
            .clone()
            .try_reserve_owned()
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => LaunchError::Full,
                mpsc::error::TrySendError::Closed(_) => LaunchError::StaleClient,
            })?;
        let intent = self.entries[&request_id].intent.clone();
        let kind = self.entries[&request_id].kind;
        self.entries
            .get_mut(&request_id)
            .ok_or(LaunchError::ReceiptUnavailable)?
            .state = LaunchState::Uncertain;
        self.receipts_changed();
        let resolved = host.resolve(&intent, kind);
        if self.check(owner, host).is_err() {
            return Err(LaunchError::StaleOwner);
        }
        self.pending.pop_front();
        let entry = self
            .entries
            .get_mut(&request_id)
            .ok_or(LaunchError::ReceiptUnavailable)?;
        match resolved {
            Ok(target) => {
                let delivery = DesktopLaunchDelivery {
                    client: token,
                    intent,
                    target: target.clone(),
                };
                if delivery.validate().is_err() {
                    entry.state = LaunchState::Failed(LaunchError::CreationUncertain);
                } else {
                    entry.target = Some(target);
                    entry.client = Some(token);
                    entry.state = LaunchState::Dispatched;
                    drop(permit.send(delivery));
                }
            }
            Err(error) => entry.state = LaunchState::Failed(error),
        }
        Ok(Some(LaunchReceipt {
            request_id,
            state: entry.state,
        }))
    }
}
