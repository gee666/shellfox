use anyhow::{ensure, Context, Result};
use shellfox_client::{Client, EventDisposition, ViewCursor};
use shellfox_protocol::*;

struct Loopback(reqwest::Client, String);

impl OutboundTransport for Loopback {
    fn send<'a>(
        &'a self,
        _: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            let transfer = async {
                let response = self
                    .0
                    .post(format!("{}{}", self.1, endpoint.path()))
                    .body(body)
                    .send()
                    .await
                    .map_err(|_| TransportError::Unavailable)?;
                if !response.status().is_success() {
                    return Err(TransportError::Rejected {
                        status: response.status().as_u16(),
                    });
                }
                let body = response
                    .bytes()
                    .await
                    .map_err(|_| TransportError::Unavailable)?;
                if body.len() > limits.max_response_bytes {
                    return Err(TransportError::TooLarge);
                }
                Ok(body.to_vec())
            };
            tokio::select! {
                _ = limits.cancel.cancelled() => Err(TransportError::Cancelled),
                result = tokio::time::timeout_at(limits.deadline.into(), transfer) => result.unwrap_or(Err(TransportError::Deadline)),
            }
        })
    }
}

#[tokio::test]
#[ignore = "requires an isolated Electron RemoteHost loopback bridge"]
async fn electron_remote_host_v1_terminal_interop() -> Result<()> {
    let origin = std::env::var("SHELLFOX_TEST_ELECTRON_V1")?;
    let url = url::Url::parse(&origin)?;
    ensure!(
        url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port().is_some()
            && url.path() == "/"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Expected a literal loopback origin."
    );
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect_policy(reqwest::redirect::Policy::none())
        .build()?;
    let peer = AuthenticatedPeerId::new("fixture-electron")?;
    let endpoint = origin.trim_end_matches('/').to_owned();
    let client = Client::new(
        peer.clone(),
        ClientId::new_v4(),
        Loopback(http.clone(), endpoint.clone()),
    );
    fn value<T>(reply: Reply<T>) -> Result<T> {
        match reply {
            Reply::Success(value) => Ok(value),
            Reply::Failure(error) => anyhow::bail!("{:?}: {}", error.code, error.message),
        }
    }
    value(client.pair().await?)?;
    let snapshot = value(client.execute(&GetSnapshot {}).await?)?;
    let tab = snapshot
        .sessions
        .first()
        .and_then(|session| session.tabs.first())
        .context("Fixture tab missing")?
        .id;
    let attach = AttachTerminal {
        tab_id: tab,
        generation: None,
        after_sequence: None,
    };
    let owned = value(client.execute(&attach).await?)?;
    let connection = client.state().connection_id;
    let mut cursor =
        ViewCursor::from_attachment(&owned, connection).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let TerminalAttachment::Owned(screen) = owned else {
        anyhow::bail!("Fixture terminal blocked");
    };
    let mut terminal = shellfox_ghostty_vt::Terminal::new(screen.cols, screen.rows)?;
    for chunk in &screen.chunks {
        terminal.feed(chunk.data.as_bytes())?;
    }
    let contents = |screen: shellfox_ghostty_vt::Screen| {
        screen
            .cells
            .into_iter()
            .map(|cell| cell.text)
            .collect::<String>()
    };
    ensure!(
        contents(terminal.render()?).contains("FIXTURE-ANSI"),
        "Initial output did not render"
    );
    let ack = cursor
        .consumed(screen.last_sequence, connection)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    value(client.execute(&ack).await?)?;
    let input = cursor
        .write("fixture-key\r".into(), Some(true), connection)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    value(client.execute(&input).await?)?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            for event in value(client.poll().await?)?.events {
                let HostEvent::Terminal { event } = event else {
                    continue;
                };
                if cursor
                    .observe(&event, connection)
                    .map_err(|e| anyhow::anyhow!("{e:?}"))?
                    != EventDisposition::Deliver
                {
                    continue;
                }
                if let TerminalEvent::Data { sequence, data, .. } = event {
                    terminal.feed(data.as_bytes())?;
                    let ack = cursor
                        .consumed(sequence, connection)
                        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
                    value(client.execute(&ack).await?)?;
                }
            }
            if contents(terminal.render()?).contains("FIXTURE-ECHO:fixture-key") {
                return Ok::<_, anyhow::Error>(());
            }
        }
    })
    .await??;
    let resize = cursor
        .resize(93, 31, connection)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    value(client.execute(&resize).await?)?;
    value(
        client
            .execute(&DetachTerminal {
                tab_id: tab,
                generation: screen.generation,
            })
            .await?,
    )?;
    let TerminalAttachment::Owned(recovered) = value(client.execute(&attach).await?)? else {
        anyhow::bail!("Reattach blocked");
    };
    ensure!(
        (recovered.cols, recovered.rows) == (93, 31),
        "Host did not retain resize"
    );
    ensure!(
        recovered.generation == screen.generation,
        "Reattach started another shell"
    );
    ensure!(
        recovered
            .chunks
            .iter()
            .any(|chunk| chunk.data.contains("FIXTURE-ECHO:fixture-key")),
        "Reattach did not replay echo"
    );
    let challenger = Client::new(peer, ClientId::new_v4(), Loopback(http, endpoint));
    value(challenger.pair().await?)?;
    ensure!(
        matches!(
            value(challenger.execute(&attach).await?)?,
            TerminalAttachment::Blocked { .. }
        ),
        "Opening a terminal transferred control implicitly"
    );
    let old_input = WriteTerminal {
        tab_id: tab,
        generation: recovered.generation,
        control_epoch: recovered.control_epoch,
        data: "unexpected after takeover".into(),
        user_initiated: Some(true),
        settled_at: None,
    };
    value(
        challenger
            .execute(&TakeTerminalControl {
                tab_id: tab,
                generation: recovered.generation,
            })
            .await?,
    )?;
    ensure!(
        matches!(
            value(challenger.execute(&attach).await?)?,
            TerminalAttachment::Owned(_)
        ),
        "Take control did not grant a writable view"
    );
    ensure!(
        matches!(client.execute(&old_input).await?, Reply::Failure(error)
        if matches!(error.code, ErrorCode::TerminalControlRequired | ErrorCode::TargetLost)),
        "The previous controller could still write after takeover"
    );
    value(challenger.disconnect().await?)?;
    value(client.disconnect().await?)?;
    Ok(())
}
