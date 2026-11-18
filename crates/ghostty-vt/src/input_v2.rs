use crate::{sys, Error, Result, Terminal};
pub use sys::{InvalidKeyV2, KeyActionV2, KeyV2 as KeyEventV2, ModifiersV2, PhysicalKey};

impl Terminal {
    /// Returns one owned encoding using the pinned public encoder. The 1 MiB allocation bound
    /// is not Runtime's 64 KiB write cap; this method never splits or writes data.
    pub fn encode_key_v2(&mut self, event: &KeyEventV2<'_>) -> Result<Vec<u8>> {
        self.engine.encode_key_v2(event).map_err(Error)
    }
}
