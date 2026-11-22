use crate::{sys, Error, Result, Terminal};
// Transitional names; removed when every caller uses `crate::key`.
pub use sys::{
    InvalidKey as InvalidKeyV2, KeyAction as KeyActionV2, KeyEvent as KeyEventV2,
    KeyModifiers as ModifiersV2, PhysicalKey,
};

impl Terminal {
    pub fn encode_key_v2(&mut self, event: &KeyEventV2<'_>) -> Result<Vec<u8>> {
        self.engine.encode_key(event).map_err(Error)
    }
}
