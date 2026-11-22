//! A 0.9.x Node host may still be running after `shellfox update`. Its CLI
//! commands now reach this binary, so `host status` and `host stop` must still
//! reach it, and `host run` must not start beside it. Transition-only: remove
//! once 0.9.x hosts can no longer be running.
use anyhow::{ensure, Context, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

#[derive(Deserialize)]
struct Endpoint {
    version: u8,
    host: String,
    port: u16,
    secret: String,
}

pub struct LegacyHost {
    endpoint: Endpoint,
    client: Client,
    id: String,
}

impl LegacyHost {
    /// Connects only when `<user_data>/host/endpoint.json` names a live host.
    pub async fn connect(user_data: &Path) -> Option<Self> {
        let bytes = std::fs::read(user_data.join("host").join("endpoint.json")).ok()?;
        let endpoint: Endpoint = serde_json::from_slice(&bytes).ok()?;
        let valid = endpoint.version == 1
            && endpoint.host == "127.0.0.1"
            && endpoint.secret.len() == 64
            && endpoint.secret.bytes().all(|b| b.is_ascii_hexdigit());
        if !valid {
            return None;
        }
        let client = Client::builder().no_proxy().build().ok()?;
        let mut host = Self {
            endpoint,
            client,
            id: String::new(),
        };
        let opened = host.request("host.open", json!({}), 10).await.ok()?;
        host.id = opened["clientId"].as_str()?.to_owned();
        Some(host)
    }

    async fn request(&self, method: &str, payload: Value, seconds: u64) -> Result<Value> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let mut body =
            json!({ "version": 1, "requestId": request_id, "method": method, "payload": payload });
        if !self.id.is_empty() {
            body["clientId"] = json!(self.id);
        }
        let url = format!("http://127.0.0.1:{}/v1/request", self.endpoint.port);
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.endpoint.secret)
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&body)?)
            .timeout(Duration::from_secs(seconds))
            .send()
            .await?;
        let reply: Value = serde_json::from_slice(&response.bytes().await?)?;
        ensure!(
            reply["requestId"] == json!(request_id),
            "Host response requestId mismatch."
        );
        if reply["ok"] == json!(true) {
            return Ok(reply["value"].clone());
        }
        anyhow::bail!(
            "{}",
            reply["error"].as_str().unwrap_or("Host request failed.")
        )
    }

    pub async fn status(&self) -> Result<Value> {
        self.request("host.status", json!({}), 30).await
    }

    /// Waits for the 0.9.x host and its terminals to end, as `host stop` did.
    pub async fn stop(&self) -> Result<()> {
        let result = self
            .request("host.stop", json!({ "wait": true }), 120)
            .await?;
        ensure!(
            result.get("stopped").is_some(),
            "Host stop was accepted but did not confirm completion."
        );
        // Its data lock is released when the process exits, shortly after the reply.
        for _ in 0..100 {
            if self.status().await.is_err() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(anyhow::anyhow!(
            "The Shellfox 0.9 host did not exit after stopping."
        ))
        .context("host stop")
    }

    pub async fn close(self) {
        let _ = self.request("host.close", json!({}), 5).await;
    }
}
