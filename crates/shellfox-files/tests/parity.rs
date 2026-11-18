#[path = "parity/asset_links.rs"]
mod asset_links;
#[path = "parity/assets.rs"]
mod assets;
#[path = "parity/directories.rs"]
mod directories;
#[path = "parity/journal.rs"]
mod journal;
#[path = "parity/streams.rs"]
mod streams;
mod support;
#[path = "parity/transfer_policy.rs"]
mod transfer_policy;
#[path = "parity/upload_ownership.rs"]
mod upload_ownership;
#[path = "parity/uploads.rs"]
mod uploads;
#[cfg(windows)]
#[path = "parity/windows_privacy.rs"]
mod windows_privacy;
