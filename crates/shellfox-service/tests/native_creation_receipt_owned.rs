//! Run only as a private public integration target with the owned OS runner.
#[path = "support/native_bootstrap_owned.rs"]
mod support;
use anyhow::{ensure, Result};
use shellfox_core::{CreateSession, ProfileId, SessionId};
use shellfox_host::{CreationRequestReceipt, HostRequest};
use shellfox_service::native::{NativeAction, NativeClient, NativeResponse};

#[test]
fn original_creation_receipt_survives_other_selection_and_deleted_is_unavailable() -> Result<()> {
    let fixture = support::Fixture::new()?;
    let data = fixture.data("creation-receipt");
    let cwd = data.to_string_lossy().into_owned();
    support::cycle(&fixture, &data, |owner, _| async move {
        let client = NativeClient::new(owner);
        let original = SessionId::new();
        let make = |request_id| CreateSession {
            request_id,
            title: None,
            cwd: cwd.clone(),
            profile_id: ProfileId::default(),
        };
        let first = make(original);
        let created = client
            .execute(NativeAction::Metadata(HostRequest::CreateSession(
                first.clone(),
            )))
            .await?;
        let NativeResponse::Snapshot(created) = created else {
            anyhow::bail!("Actual create snapshot absent");
        };
        let actual = created
            .selection
            .session_id
            .ok_or_else(|| anyhow::anyhow!("Created selection absent"))?;
        ensure!(actual != original, "Host ID contract changed");
        ensure!(
            client.creation_request_receipt(original).await?
                == CreationRequestReceipt::SessionCreated { session_id: actual },
            "Authoritative original receipt missing"
        );
        let second = client
            .execute(NativeAction::Metadata(HostRequest::CreateSession(make(
                SessionId::new(),
            ))))
            .await?;
        let NativeResponse::Snapshot(second) = second else {
            anyhow::bail!("Second snapshot absent");
        };
        ensure!(second.selection.session_id != Some(actual));
        let retry = client
            .execute(NativeAction::Metadata(HostRequest::CreateSession(first)))
            .await?;
        let NativeResponse::Snapshot(retry) = retry else {
            anyhow::bail!("Retry snapshot absent");
        };
        ensure!(
            retry.selection == second.selection,
            "Test must exercise unrelated selection"
        );
        ensure!(
            client.creation_request_receipt(original).await?
                == CreationRequestReceipt::SessionCreated { session_id: actual },
            "Retry incorrectly followed selection"
        );
        ensure!(
            client.creation_request_receipt(SessionId::new()).await?
                == CreationRequestReceipt::ReceiptUnavailable
        );
        client
            .execute(NativeAction::DeleteSession { session: actual })
            .await?;
        ensure!(
            client.creation_request_receipt(original).await?
                == CreationRequestReceipt::ReceiptUnavailable,
            "Deleted receipt invented a focus target"
        );
        Ok(())
    })?;
    Ok(())
}
