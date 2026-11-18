use super::AdapterError;
use std::{io, time::Duration};

pub enum HelperFrame {
    Line(Vec<u8>),
    InputClosed,
    Eof,
}

/// Trusted host injection for private inherited stdio only. The C# launcher
/// authenticates its elevated peer with original handles and birth. Never expose
/// this trait or arbitrary pipe/resource selection through renderer IPC.
pub trait HelperIo {
    /// WouldBlock means no bytes were accepted. Every other failure can represent
    /// partial delivery and must not be replayed.
    fn send_line(&mut self, line: &[u8]) -> io::Result<()>;
    fn try_receive_line(&mut self) -> io::Result<Option<HelperFrame>>;
    fn queued_write_bytes(&self) -> usize;
    fn service(&mut self) -> Result<(), AdapterError> {
        Ok(())
    }
    fn disconnect_input(&mut self);
    fn distrust_output(&mut self) {}
    /// Called before attempting a launch frame, including unknown partial sends.
    fn launch_started(&mut self) {}
    /// Isolated protocol final exit includes elevated peer HANDLE exit proof.
    fn peer_exit_confirmed(&mut self) {}
    /// Requires the protocol proof first; native implementations also join pipes
    /// and confirm the original launcher. Timeout retains every remaining owner.
    fn finish(&mut self, _budget: Duration) -> Result<(), AdapterError> {
        Ok(())
    }
}
