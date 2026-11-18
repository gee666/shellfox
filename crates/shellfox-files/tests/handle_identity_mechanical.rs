//! Pure identity equality mechanics, no native API/filesystem/ReFS acceptance.
#[path = "../src/error.rs"]
mod error;
pub use error::{ErrorCode, FileError, Result};
#[path = "../src/private_identity.rs"]
mod provider;
pub use provider::HandleIdentity;
const _: fn(&cap_std::fs::Dir) -> Result<HandleIdentity> = HandleIdentity::from_directory;
const _: fn(&cap_std::fs::File) -> Result<HandleIdentity> = HandleIdentity::from_cap_file;
