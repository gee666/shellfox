//! Existing private fixture helpers. Production boot uses native_preparation.
use super::*;
impl PrivateDirectory {
    pub async fn prepare(
        data: &Path,
        helper: &Path,
        digest: [u8; 32],
        owner: bool,
    ) -> Result<Self> {
        Self::prepare_reported(data, helper, digest, owner, || {}).await
    }
    pub async fn prepare_reported(
        data: &Path,
        helper: &Path,
        digest: [u8; 32],
        owner: bool,
        before_handles: impl FnOnce(),
    ) -> Result<Self> {
        let mut private = Self::plan(data, helper, digest, owner)?;
        let mut preparation = shellfox_transport::OfflinePreparation::new(private.helper.clone())?;
        before_handles();
        let result = async {
            private.prepare_root(&mut preparation, owner).await?;
            if !owner {
                private
                    .check(
                        &private.path,
                        true,
                        false,
                        preparation.permission_verifier(),
                    )
                    .await?;
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if result.is_err() {
            private.creation_owner().preserve_stale()?;
        }
        loop {
            match preparation.shutdown_confirmed().await {
                Ok(_proof) => break,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            }
        }
        result?;
        Ok(private)
    }
}
