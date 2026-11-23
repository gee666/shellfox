use super::*;
impl Manager {
    pub(super) async fn cleanup_confirmed(&mut self) {
        self.state.enabled = false;
        self.state.login_url = None;
        self.state.attempts.clear();
        self.state.cleanup_retained = true;
        self.state.error = Some(cleanup_retained());
        self.state.status = RemoteNetworkStatus::Error;
        self.publish.send_replace(self.state.clone());
        self.cycle.cancel();
        self.jobs.shutdown().await;
        // Do not abort a helper-start/stop future: it owns a child even before
        // returning its transport handle. Adoption is forbidden during cleanup.
        while let Some(event) = self.owned_jobs.join_next().await {
            match event {
                Ok(Event::Started {
                    previous_confirmed,
                    result,
                    ..
                }) => {
                    if previous_confirmed {
                        self.network.take();
                    }
                    if let Ok(network) = result {
                        self.orphan = Some(network);
                    }
                }
                Ok(event) => self.lifecycle_event(event).await,
                Err(_) => self.owned_failed = true,
            }
        }
        for peer in &mut self.state.peers {
            peer.connection = RemoteConnection::Disconnected;
            peer.attempt_id = None;
        }
        loop {
            let gate = self.backend.approvals().set_enabled_async(false).await;
            if let Some(network) = self.network.as_ref() {
                if matches!(
                    tokio::time::timeout(Duration::from_secs(5), network.shutdown()).await,
                    Ok(Ok(()))
                ) {
                    self.network.take();
                }
            }
            if let Some(network) = self.orphan.as_ref() {
                if matches!(
                    tokio::time::timeout(Duration::from_secs(5), network.shutdown()).await,
                    Ok(Ok(()))
                ) {
                    self.orphan.take();
                }
            }
            if gate.is_ok() && self.network.is_none() && self.orphan.is_none() && !self.owned_failed
            {
                self.state.cleanup_retained = false;
                self.state.status = RemoteNetworkStatus::Disabled;
                self.state.error = None;
                self.emit().await;
                return;
            }
            self.state.cleanup_retained = true;
            self.state.status = RemoteNetworkStatus::Error;
            self.state.error = Some(cleanup_retained());
            self.emit().await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
