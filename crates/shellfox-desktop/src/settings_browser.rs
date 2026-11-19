//! A browser is a local user-interface action, not remote attempt continuation.
use super::*;
fn dispatch(url: &str, open: impl FnOnce(&str)) -> anyhow::Result<()> {
    let validated = shellfox_service::system::validate_web_url(url)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    open(&validated);
    Ok(())
}
impl SettingsPanel {
    pub(super) fn dispatch_login_browser(&mut self, serial: u64, cx: &mut Context<Self>) {
        if self.operations.frozen() || !self.remote.login.is_current(serial) {
            return;
        }
        let result = self
            .remote
            .login
            .url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No current sign-in address."))
            .and_then(|url| dispatch(url, |url| cx.open_url(url)));
        if result.is_err() {
            self.remote.login.browser_failed();
        }
        // GPUI dispatches to the local platform. Its API has no completion ACK;
        // this never emits Applied/Opened or claims authentication/readiness.
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_browser_dispatch_validates_before_injected_opener_without_owner_rpc() {
        let mut calls = Vec::new();
        for url in [
            "javascript:alert(1)",
            "file:///tmp/no",
            "https://example.invalid/\n",
        ] {
            assert!(dispatch(url, |url| calls.push(url.to_owned())).is_err());
        }
        assert!(calls.is_empty());
        dispatch("https://login.example.invalid/owned", |url| {
            calls.push(url.to_owned())
        })
        .unwrap();
        assert_eq!(calls, ["https://login.example.invalid/owned"]);
    }
}
