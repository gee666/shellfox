//! Remote-only mobile bindings. No PTY, host, helper process, filesystem or login.
mod abi;
mod client;
mod configure;
mod engine;
mod frame;
mod composer;
mod input;
mod interaction;
mod queue;

pub use abi::*;
pub use client::*;
pub use configure::sfm_terminal_configure;
pub use engine::Status;
pub use frame::{SfmCell, SfmFrame};
pub use interaction::*;

pub const MAX_CHUNK: usize = 256 * 1024;
pub const MAX_QUEUE: usize = 4 * 1024 * 1024;
pub const MAX_CHUNKS: usize = 64;
pub const MAX_INPUT: usize = 64 * 1024;
pub const MAX_PUMP: usize = 512 * 1024;
