//! Two retained production bootstraps; the existing owned harness remains the custodian.
use super::support::{runtime, Fixture, Reports, StopOnDrop};
use anyhow::{Context, Result};
use shellfox_service::{
    local::{LocalBootstrap, RemoteBootOptions},
    OwnerClient,
};
use std::{future::Future, path::Path};

pub fn two<T, F, Work>(
    fixture: &Fixture,
    a: &Path,
    b: &Path,
    options: [RemoteBootOptions; 2],
    driver: F,
) -> Result<T>
where
    F: FnOnce(OwnerClient, OwnerClient) -> Work,
    Work: Future<Output = Result<T>>,
{
    let rt = runtime()?;
    let mut first = LocalBootstrap::plan(a, &fixture.helper, fixture.digest)?;
    let mut second = LocalBootstrap::plan(b, &fixture.helper, fixture.digest)?;
    let first_stop = first.stop_handle();
    let second_stop = second.stop_handle();
    let first_reports = Reports::default();
    let second_reports = Reports::default();
    let first_callback = first_reports.clone();
    let second_callback = second_reports.clone();
    let [first_options, second_options] = options;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rt.block_on(async {
            let (send_a, ready_a) = tokio::sync::oneshot::channel();
            let (send_b, ready_b) = tokio::sync::oneshot::channel();
            let run_a = first.run_remote(
                first_options,
                move |owner| {
                    let _ = send_a.send(owner);
                },
                move |r| first_callback.send(r),
            );
            let run_b = second.run_remote(
                second_options,
                move |owner| {
                    let _ = send_b.send(owner);
                },
                move |r| second_callback.send(r),
            );
            let work = async {
                let _first = StopOnDrop(first_stop);
                let _second = StopOnDrop(second_stop);
                let a = ready_a.await.context("First actual readiness absent")?;
                let b = ready_b.await.context("Second actual readiness absent")?;
                driver(a, b).await
            };
            let (served_a, served_b, driven) = tokio::join!(run_a, run_b, work);
            served_a?;
            served_b?;
            driven
        })
    }));
    let result = match outcome {
        Ok(value) => value,
        Err(_) => Err(first.retain_error(anyhow::anyhow!(
            "Two-host driver unwind; originals retained"
        ))),
    };
    if first_reports.clean().is_err() {
        rt.block_on(first.cleanup(|r| first_reports.send(r)));
    }
    if second_reports.clean().is_err() {
        rt.block_on(second.cleanup(|r| second_reports.send(r)));
    }
    first_reports.clean()?;
    second_reports.clean()?;
    eprintln!("Both original Go/Service owners: actual Stop/drain/NoOwnerClean confirmed before driver result.");
    result
}
