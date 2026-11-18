use super::common::Fixture;
use anyhow::Result;
use shellfox_service::{local::LocalBootStop, OwnerClient};
use std::{future::Future, path::Path};

pub fn cycle<T, F, Work>(fixture: &Fixture, data: &Path, driver: F) -> Result<T>
where
    F: FnOnce(OwnerClient, LocalBootStop) -> Work,
    Work: Future<Output = Result<T>>,
{
    super::common::cycle_inner(fixture, data, None, driver)
}
