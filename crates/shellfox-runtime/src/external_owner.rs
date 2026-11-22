use crate::{
    external::{ExternalMessage, ExternalOwner},
    owner::Owner,
    *,
};
use std::sync::mpsc::TryRecvError;
impl ExternalOwner {
    pub(crate) fn close(&mut self, finished: bool, failed: bool) {
        self.receiver.take();
        let mut status = self
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        status.finished |= finished;
        status.failed |= failed;
    }
}
impl Owner {
    /// Same contract as poll_output.
    pub(crate) fn poll_external(&mut self, budget: usize) -> bool {
        for _ in 0..budget {
            let received = match self
                .external
                .as_ref()
                .and_then(|source| source.receiver.as_ref())
            {
                Some(receiver) => receiver.try_recv(),
                None => return false,
            };
            match received {
                Ok(ExternalMessage::Data(chunk)) => {
                    if let Err(error) = self.external_data(chunk) {
                        self.fail(error);
                        return false;
                    }
                }
                Ok(ExternalMessage::Finish(last)) => {
                    if let Err(error) = self.finish_external(last) {
                        self.fail(error);
                    }
                    return false;
                }
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => {
                    self.fail(RuntimeError::unavailable(
                        "External source disconnected without ordered finish.",
                    ));
                    return false;
                }
            }
        }
        true
    }
    fn finish_external(&mut self, last: SourceSequence) -> Result<(), RuntimeError> {
        let source = self
            .external
            .as_mut()
            .ok_or_else(|| RuntimeError::validation("External source missing."))?;
        if source
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .source_sequence
            != last
        {
            return Err(RuntimeError::validation(
                "External finish does not match processed source bytes.",
            ));
        }
        let final_display = source.display.finish();
        let delivery = if final_display.is_empty() {
            None
        } else {
            let (first, last) = self.accept_source_bytes(&final_display)?;
            Some(DeliveryRange { first, last })
        };
        let source = self
            .external
            .as_mut()
            .ok_or_else(|| RuntimeError::validation("External source missing."))?;
        {
            let mut status = source
                .status
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            status.finish_delivery = delivery;
            status.last_delivery_sequence = DeliverySequence(self.delivery.current_sequence());
        }
        source.close(true, false);
        // End the SOURCE, not the owner command loop or canonical parser.
        self.set_phase(Phase::Closed, None);
        Ok(())
    }
    fn external_data(&mut self, chunk: ExternalChunk) -> Result<(), RuntimeError> {
        let source = self
            .external
            .as_ref()
            .ok_or_else(|| RuntimeError::validation("External source missing."))?;
        let prior = source
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .source_sequence;
        if prior.0.checked_add(1) != Some(chunk.sequence.0)
            || chunk.bytes.is_empty()
            || chunk.bytes.len() > EXTERNAL_CHUNK_BYTES
        {
            return Err(RuntimeError::validation(
                "External source sequence/size is invalid.",
            ));
        }
        let display = self
            .external
            .as_mut()
            .ok_or_else(|| RuntimeError::validation("External source missing."))?
            .display
            .data(chunk.stream, &chunk.bytes);
        let delivery = if display.is_empty() {
            None
        } else {
            let (first, last) = self.accept_source_bytes(&display)?;
            Some(DeliveryRange { first, last })
        };
        let source = self
            .external
            .as_ref()
            .ok_or_else(|| RuntimeError::validation("External source missing."))?;
        let mut status = source
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        status.source_sequence = chunk.sequence;
        status.stream = Some(chunk.stream);
        status.delivery = delivery;
        status.last_delivery_sequence = DeliverySequence(self.delivery.current_sequence());
        Ok(())
    }
}
