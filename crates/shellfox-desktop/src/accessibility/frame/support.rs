use super::*;

impl SemanticFrame {
    /// Attach the sampled terminal view as private action metadata, never Node text/properties.
    /// Call during terminal paint using the emitted snapshot's stamp, not a newer getter.
    pub fn set_terminal_view(
        &self,
        id: NodeId,
        stamp: super::super::TerminalViewStamp,
    ) -> Result<(), SemanticError> {
        let mut data = self.data.borrow_mut();
        if data.phase != Phase::Collecting {
            return Err(SemanticError::FrameNotCollecting);
        }
        if data.terminal_views.len() >= super::super::tree::MAX_NODES
            && !data.terminal_views.contains_key(&id)
        {
            data.error.get_or_insert(SemanticError::Limit);
            return Err(SemanticError::Limit);
        }
        if let Some(old) = data.terminal_views.get(&id)
            && *old != stamp
        {
            data.error.get_or_insert(SemanticError::TerminalStamp(id));
            return Err(SemanticError::TerminalStamp(id));
        }
        data.terminal_views.insert(id, stamp);
        Ok(())
    }

    pub(in crate::accessibility) fn is_current(&self, generation: &FrameGeneration) -> bool {
        let data = self.data.borrow();
        generation.owner.ptr_eq(&Rc::downgrade(&self.data))
            && data.generation == generation.generation
            && data.phase == Phase::Collecting
    }
}
