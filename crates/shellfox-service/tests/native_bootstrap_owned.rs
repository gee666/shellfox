//! Owned offline native boot acceptance. Real current Go helper; no tailnet/accounts.
#[path = "support/native_bootstrap_owned.rs"]
mod support;
use anyhow::{Context, Result};
use shellfox_service::{
    experiment::ExperimentRequest,
    local::{LocalBootstrap, LocalClient, OccupiedOwnerError},
};
use std::{
    future::{poll_fn, Future},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::Poll,
};
use support::*;

#[test]
fn real_go_cold_existing_and_occupied_share_one_owner() -> Result<()> {
    let mut fixture = Fixture::new()?;
    let data = fixture.data("cold/nested/host");
    let path = data.clone();
    let helper = fixture.helper.clone();
    let digest = fixture.digest;
    cycle(&fixture, &data, move |client, _stop| async move {
        client.execute(ExperimentRequest::Snapshot {}).await?;
        let mut occupied = LocalBootstrap::plan(&path, &helper, digest)?;
        let reports = Reports::default();
        let ready = Arc::new(AtomicBool::new(false));
        let observed = ready.clone();
        let result = occupied
            .run_with_dispatcher(
                (),
                move |_| {
                    observed.store(true, Ordering::Release);
                },
                |r| reports.send(r),
            )
            .await;
        reports.clean()?;
        let error = result.expect_err("actual existing Go owner must win");
        anyhow::ensure!(
            error.is::<OccupiedOwnerError>(),
            "not the sealed occupied outcome: {error:#}"
        );
        anyhow::ensure!(
            !ready.load(Ordering::Acquire),
            "contender activated another Host"
        );
        client.execute(ExperimentRequest::Snapshot {}).await?;
        Ok(())
    })?;
    cycle(&fixture, &data, |client, _| async move {
        client.execute(ExperimentRequest::Snapshot {}).await?;
        Ok(())
    })?;
    fixture.success();
    Ok(())
}
#[test]
fn preparation_collision_preserves_original_error_without_repair() -> Result<()> {
    let mut fixture = Fixture::new()?;
    let data = fixture.data("collision");
    let runtime = runtime()?;
    let mut boot = LocalBootstrap::plan(&data, &fixture.helper, fixture.digest)?;
    std::fs::write(&data, b"owned collision, preserve")?;
    let reports = Reports::default();
    let result = runtime.block_on(boot.run_with_dispatcher((), |_| {}, |r| reports.send(r)));
    reports.clean()?;
    let error = result.expect_err("changed original plan must fail");
    anyhow::ensure!(
        !format!("{error:#}").contains("Invalid helper path form"),
        "empty registration masked the actual root error"
    );
    anyhow::ensure!(
        std::fs::read(&data)? == b"owned collision, preserve",
        "collision was repaired or replaced"
    );
    fixture.success();
    Ok(())
}
#[test]
fn real_go_failed_actor_start_cleans_original_controller() -> Result<()> {
    let mut fixture = Fixture::new()?;
    let data = fixture.data("failed-host");
    cycle(&fixture, &data, |client, _| async move {
        client.execute(ExperimentRequest::Snapshot {}).await?;
        Ok(())
    })?;
    std::fs::write(
        data.join(shellfox_host::STORE_FILENAME),
        b"owned corrupt SQLite fixture",
    )?;
    let runtime = runtime()?;
    let mut boot = LocalBootstrap::plan(&data, &fixture.helper, fixture.digest)?;
    let reports = Reports::default();
    let ready = Arc::new(AtomicBool::new(false));
    let observed = ready.clone();
    let result = runtime.block_on(boot.run_with_dispatcher(
        (),
        move |_| {
            observed.store(true, Ordering::Release);
        },
        |r| reports.send(r),
    ));
    reports.clean()?;
    anyhow::ensure!(!ready.load(Ordering::Acquire), "failed actor became ready");
    let error = result.expect_err("owned corrupt SQLite startup must fail");
    anyhow::ensure!(
        error.chain().any(|error| error
            .downcast_ref::<shellfox_core::Error>()
            .is_some_and(|e| e.code == shellfox_core::ErrorCode::StorageFailed)),
        "not an actual Host storage failure after acquisition: {error:#}"
    );
    fixture.success();
    Ok(())
}
#[test]
fn real_go_cancelled_and_unwound_preparation_keep_same_cleanup() -> Result<()> {
    let mut fixture = Fixture::new()?;
    for unwind in [false, true] {
        let data = fixture.data(if unwind { "unwind" } else { "cancel" });
        let runtime = runtime()?;
        let mut boot = LocalBootstrap::plan(&data, &fixture.helper, fixture.digest)?;
        let reports = Reports::default();
        let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.block_on(async {
                let mut work = Box::pin(boot.run_with_dispatcher((), |_| {}, |r| reports.send(r)));
                let pending =
                    poll_fn(|cx| Poll::Ready(matches!(work.as_mut().poll(cx), Poll::Pending)))
                        .await;
                if pending && unwind {
                    panic!("owned deliberate unwind with original bootstrap retained");
                }
                pending
            })
        }));
        runtime.block_on(boot.cleanup(|r| reports.send(r)));
        reports.clean()?;
        if unwind {
            anyhow::ensure!(first.is_err(), "did not reach in-flight native preparation");
        } else {
            anyhow::ensure!(
                matches!(first, Ok(true)),
                "did not reach cancellable native work"
            );
        }
    }
    fixture.success();
    Ok(())
}
struct PausedActor {
    entered: Option<tokio::sync::oneshot::Sender<()>>,
    resume: std::sync::mpsc::Receiver<()>,
}
impl shellfox_service::OwnerDispatcher for PausedActor {
    fn dispatch_one(&mut self, _: &mut shellfox_service::Service) {
        if let Some(entered) = self.entered.take() {
            let _ = entered.send(());
            let _ = self.resume.recv();
        }
    }
}
#[test]
fn real_go_cancelled_stop_cleanup_resumes_original_owner() -> Result<()> {
    let mut fixture = Fixture::new()?;
    let data = fixture.data("cancelled-stop");
    let runtime = runtime()?;
    let mut boot = LocalBootstrap::plan(&data, &fixture.helper, fixture.digest)?;
    let stop = boot.stop_handle();
    let reports = Reports::default();
    let (entered, paused) = tokio::sync::oneshot::channel();
    let (resume, released) = std::sync::mpsc::channel();
    let (ready, received) = tokio::sync::oneshot::channel();
    let observed = runtime.block_on(async {
        let mut run = Box::pin(boot.run_with_dispatcher(
            PausedActor {
                entered: Some(entered),
                resume: released,
            },
            move |owner| {
                let _ = ready.send(owner);
            },
            |r| reports.send(r),
        ));
        let drive = async {
            let _ = received.await?;
            paused.await?;
            stop.request();
            Ok::<_, anyhow::Error>(())
        };
        tokio::pin!(drive);
        let mut drive_done = false;
        // Includes real cold preparation before Ready; cancellation still waits
        // for the actual post-Ready RetainedCleanup report with actor held.
        tokio::time::timeout(
            std::time::Duration::from_secs(45),
            poll_fn(|cx| {
                if let Poll::Ready(result) = run.as_mut().poll(cx) {
                    return Poll::Ready(Err(result
                        .err()
                        .unwrap_or_else(|| anyhow::anyhow!("run unexpectedly completed"))));
                }
                if !drive_done {
                    match drive.as_mut().poll(cx) {
                        Poll::Ready(Ok(())) => drive_done = true,
                        Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                        Poll::Pending => {}
                    }
                }
                if reports.cleanup_started_after_ready() {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                }
            }),
        )
        .await
        // run is dropped HERE with the actor intentionally still held by its real controller.
    });
    let not_clean = reports.clean().is_err();
    let _ = resume.send(());
    runtime.block_on(boot.cleanup(|r| reports.send(r)));
    reports.clean()?;
    observed.context("stop cleanup did not begin")??;
    anyhow::ensure!(not_clean, "unconfirmed blocked actor was falsely clean");
    fixture.success();
    Ok(())
}
#[test]
fn real_go_direct_stop_ignores_missing_endpoint_and_drains_connected_lifetime() -> Result<()> {
    let mut fixture = Fixture::new()?;
    let data = fixture.data("direct-stop");
    let path = data.clone();
    let helper = fixture.helper.clone();
    let digest = fixture.digest;
    cycle(&fixture, &data, move |client, stop| async move {
        let external = LocalClient::connect(&path, &helper, digest).await?;
        external.execute(ExperimentRequest::Snapshot {}).await?;
        let endpoint = path.join("rust-service-v1/endpoint.json");
        std::fs::rename(
            &endpoint,
            path.join("rust-service-v1/owned-hidden-endpoint.json"),
        )
        .context("owned endpoint-discovery fault injection")?;
        client.execute(ExperimentRequest::Snapshot {}).await?;
        stop.request();
        // Keep the real authenticated external lifetime alive across Stop.
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        drop(external);
        Ok(())
    })?;
    fixture.success();
    Ok(())
}
