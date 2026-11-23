use super::*;
impl Manager {
    pub(super) fn discover(&mut self) {
        if self.probe_busy || self.jobs.len() >= 48 {
            return;
        }
        let Some(network) = self.network.clone() else {
            return;
        };
        let names: Vec<_> = self
            .state
            .peers
            .iter()
            .filter(|peer| {
                peer.online
                    && self
                        .discovery
                        .get(&peer.id)
                        .is_none_or(|checked| checked.elapsed() >= Duration::from_secs(300))
            })
            .take(16)
            .map(|peer| peer.id.clone())
            .collect();
        if names.is_empty() {
            return;
        }
        self.probe_busy = true;
        let epoch = self.epoch;
        let cancel = self.cycle.clone();
        self.spawn_job(async move {
            let mut jobs = tokio::task::JoinSet::new();
            for name in names {
                let network = network.clone();
                let cancel = cancel.child_token();
                jobs.spawn(async move {
                    let result = async {
                        let peer = wire::AuthenticatedPeerId::new(name.clone()).map_err(|_| {
                            error(ErrorCode::Validation, "Invalid discovered peer.")
                        })?;
                        peers::success(
                            peers::send::<peers::Discovery>(
                                network.as_ref(),
                                &peer,
                                wire::Endpoint::Discover,
                                Uuid::new_v4(),
                                cancel,
                            )
                            .await?,
                        )?;
                        Ok::<_, AppError>(())
                    }
                    .await;
                    (name, result.is_ok())
                });
            }
            let mut found = vec![];
            while let Some(result) = jobs.join_next().await {
                if let Ok(result) = result {
                    found.push(result);
                }
            }
            Event::Discovered {
                epoch,
                peers: found,
            }
        });
    }
}
