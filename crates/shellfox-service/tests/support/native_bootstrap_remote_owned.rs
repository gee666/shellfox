#[path = "native_bootstrap_common_owned.rs"]
pub(crate) mod common;
#[path = "native_bootstrap_local_cycle.rs"]
mod local_cycle;
pub use common::*;
pub use local_cycle::cycle;

use anyhow::Result;
use shellfox_service::{local::LocalBootStop, OwnerClient};
use std::{future::Future, path::Path};

pub fn cycle_remote<T, F, Work>(
    fixture: &Fixture,
    data: &Path,
    remote: shellfox_service::local::RemoteBootOptions,
    driver: F,
) -> Result<T>
where
    F: FnOnce(OwnerClient, LocalBootStop) -> Work,
    Work: Future<Output = Result<T>>,
{
    common::cycle_inner(fixture, data, Some(remote), driver)
}
