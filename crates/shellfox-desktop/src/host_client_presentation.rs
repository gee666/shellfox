use super::*;

impl HostClient {
    pub async fn frame(&self, tab: TabId, attachment: Attachment) -> Result<Option<NativeFrame>> {
        self.frame_focused(tab, attachment, true).await
    }
    pub async fn frame_focused(
        &self,
        tab: TabId,
        attachment: Attachment,
        focused: bool,
    ) -> Result<Option<NativeFrame>> {
        // A bounded fair queue avoids starving later hidden tabs, while only
        // two frame cycles compete with ordered key/release admission.
        NativeAction::Presentation {
            tab,
            attachment: NativeViewAttachment::Controller(attachment.clone()),
            action: NativePresentation::Frame { focused },
        }
        .validate()
        .map_err(|error| NativeAdmissionError::Invalid(error.to_string()))?;
        let _frame_admission =
            match Admission::acquire_limit(self.frame_admission.clone(), MAX_FRAME_WAITERS) {
                Ok(slot) => slot,
                Err(_) => {
                    return Err(NativeAdmissionError::Full(Box::new(NativeAction::Frame {
                        tab,
                        attachment,
                    }))
                    .into());
                }
            };
        let _permit = self
            .frame_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| {
                NativeAdmissionError::Stopped(Box::new(NativeAction::Frame {
                    tab,
                    attachment: attachment.clone(),
                }))
            })?;
        let NativeResponse::Frame(frame) = self
            .execute(NativeAction::Presentation {
                tab,
                attachment: NativeViewAttachment::Controller(attachment.clone()),
                action: NativePresentation::Frame { focused },
            })
            .await?
        else {
            anyhow::bail!("Service returned an invalid native frame response.");
        };
        let Some(frame) = frame else {
            return Ok(None);
        };
        ensure!(
            frame.frame.identity.tab_id == tab
                && frame.frame.identity.generation == attachment.generation
                && matches!(frame.presentation.view, shellfox_runtime::PresentationLease::Controller(lease) if lease.controller == attachment.controller && lease.epoch == attachment.epoch),
            "Native frame identity/view mismatch."
        );
        self.frames
            .lock()
            .map_err(|_| anyhow::anyhow!("Native frame cache poisoned."))?
            .accept(tab, false, frame.clone())?;
        // Bounded native state has accepted the immutable frame. Painting is not
        // the consumption boundary, and receive/enqueue alone never sends ACKs.
        if !frame.replay_lost {
            self.execute(NativeAction::AcceptFrame {
                tab,
                attachment,
                frame_id: frame.frame_id,
            })
            .await?;
        }
        Ok(Some(frame))
    }
    pub async fn observer_frame(
        &self,
        tab: TabId,
        attachment: NativeObserverAttachment,
    ) -> Result<Option<NativeFrame>> {
        self.observer_frame_focused(tab, attachment, true).await
    }
    pub async fn observer_frame_focused(
        &self,
        tab: TabId,
        attachment: NativeObserverAttachment,
        focused: bool,
    ) -> Result<Option<NativeFrame>> {
        NativeAction::ObserverFrame {
            tab,
            attachment: attachment.clone(),
        }
        .validate()
        .map_err(|error| NativeAdmissionError::Invalid(error.to_string()))?;
        let _frame_admission =
            match Admission::acquire_limit(self.frame_admission.clone(), MAX_FRAME_WAITERS) {
                Ok(slot) => slot,
                Err(_) => {
                    return Err(NativeAdmissionError::Full(Box::new(
                        NativeAction::ObserverFrame { tab, attachment },
                    ))
                    .into());
                }
            };
        let _permit = self
            .frame_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| {
                NativeAdmissionError::Stopped(Box::new(NativeAction::ObserverFrame {
                    tab,
                    attachment: attachment.clone(),
                }))
            })?;
        let NativeResponse::Frame(frame) = self
            .execute(NativeAction::Presentation {
                tab,
                attachment: NativeViewAttachment::Observer(attachment.clone()),
                action: NativePresentation::Frame { focused },
            })
            .await?
        else {
            anyhow::bail!("Service returned an invalid observer frame response.");
        };
        let Some(frame) = frame else {
            return Ok(None);
        };
        ensure!(
            frame.frame.identity.tab_id == tab
                && frame.frame.identity.generation == attachment.generation
                && matches!(frame.presentation.view, shellfox_runtime::PresentationLease::Observer(lease) if lease.viewer == attachment.viewer && lease.epoch == attachment.epoch),
            "Observer frame identity/view mismatch."
        );
        self.frames
            .lock()
            .map_err(|_| anyhow::anyhow!("Native frame cache poisoned."))?
            .accept(tab, true, frame.clone())?;
        if !frame.replay_lost {
            self.execute(NativeAction::AcceptObserverFrame {
                tab,
                attachment,
                frame_id: frame.frame_id,
            })
            .await?;
        }
        Ok(Some(frame))
    }
    pub fn latest(&self, tab: TabId, observer: bool) -> Result<Option<NativeFrame>> {
        Ok(self
            .frames
            .lock()
            .map_err(|_| anyhow::anyhow!("Native frame cache poisoned."))?
            .latest(tab, observer))
    }
}
