//! Explicitly authorized Windows-key to owned Linux-project workflow only.
#![cfg(windows)]
use super::*;
use std::time::{Duration, Instant};
const PROJECT: &str = "/home/oira666/shellfox-ghostti-migration.8lza8G";
const PIN: &str = "OX/aKcOXSLWu+OzsX2Gy9oPdnoh7E1xGBoSt+ZpKG2E=";
async fn request(client: &NativeClient, action: NativeAction) -> Result<NativeResponse> {
    tokio::time::timeout(Duration::from_secs(20), client.execute(action))
        .await
        .map_err(|_| anyhow::anyhow!("owned SSH operation deadline; original cleanup requested"))?
}
async fn status(client: &NativeClient, connection: uuid::Uuid) -> Result<ssh::ConnectionStatus> {
    let NativeResponse::Ssh(ssh::Response::Status(mut rows)) = request(
        client,
        NativeAction::Ssh(ssh::Request::GetStatus {
            connection: Some(connection),
        }),
    )
    .await?
    else {
        anyhow::bail!("SSH status response missing");
    };
    ensure!(rows.len() == 1, "original SSH connection receipt missing");
    Ok(rows.remove(0))
}
#[test]
fn authorized_key_session_uses_pinned_host_and_only_owned_project_commands() -> Result<()> {
    let mut fixture = support::Fixture::new()?;
    let data = fixture.data("authorized-key-session");
    let (providers, _recorder) = inputs::providers(&data, &fixture.helper, fixture.digest)?;
    let cli = inputs::program("CLI")?;
    let options = app_composition::NativeApplicationConfig::new(
        &data,
        None,
        Some("owned-ssh-session".into()),
        app_composition::SourceFactory::Local,
    )?
    .remote_options(cli.path().to_owned(), false)?
    .with_native_providers(providers)?;
    support::cycle_remote(&fixture, &data, options, |owner, _| async move {
        let client = NativeClient::new(owner);
        let profile = uuid::Uuid::new_v4();
        // PATH ONLY. Neither this test nor any tool reads/copies the key contents.
        // The authorized application/OpenSSH authentication path may use it.
        request(
            &client,
            NativeAction::Ssh(ssh::Request::Save(ssh::Input {
                id: profile,
                name: "authorized-project-only".into(),
                host: "192.168.0.107".into(),
                port: 22,
                user: "oira666".into(),
                password: ssh::Password::Clear,
                key_file: Some("C:/Users/user/.ssh/id_ed25519_omarchy".into()),
                remote_cwd: Some(PROJECT.into()),
            })),
        )
        .await?;
        let NativeResponse::SshOpened(opened) = request(
            &client,
            NativeAction::SshOpen(ssh::OpenProfile {
                request_id: shellfox_core::SessionId::new(),
                profile,
                cols: 120,
                rows: 40,
            }),
        )
        .await?
        else {
            anyhow::bail!("original SSH open receipt missing");
        };
        let connection = opened.connection.connection;
        let tab = opened.connection.tab;
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut confirmed = false;
        loop {
            let current = status(&client, connection).await?;
            match current.state {
                ssh::State::AwaitingHost(challenge) => {
                    ensure!(!confirmed, "unexpected repeated host confirmation");
                    ensure!(
                        challenge.host == "192.168.0.107" && challenge.port == 22,
                        "unexpected target"
                    );
                    ensure!(
                        challenge
                            .fingerprint
                            .strip_prefix("SHA256:")
                            .unwrap_or(&challenge.fingerprint)
                            .trim_end_matches('=')
                            == PIN.trim_end_matches('='),
                        "provided host pin differs; no trust change permitted"
                    );
                    request(
                        &client,
                        NativeAction::Ssh(ssh::Request::ConfirmHost {
                            connection,
                            challenge: challenge.challenge,
                            decision: ssh::TrustDecision::TrustAndSave,
                        }),
                    )
                    .await?;
                    confirmed = true;
                }
                ssh::State::Ready => {
                    ensure!(
                        confirmed && current.runtime.is_some(),
                        "Ready lacks original authentication/runtime proof"
                    );
                    break;
                }
                ssh::State::Failed { message } => {
                    anyhow::bail!("authorized SSH preparation/authentication failed: {message}")
                }
                ssh::State::AwaitingPrompt(_) => {
                    anyhow::bail!("additional secret prompt required; no secret was supplied")
                }
                ssh::State::Closed => anyhow::bail!("SSH closed before Ready"),
                _ => {}
            }
            ensure!(Instant::now() < deadline, "SSH Ready deadline");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let NativeResponse::Attached(attachment) = request(
            &client,
            NativeAction::Attach {
                tab,
                takeover: false,
            },
        )
        .await?
        else {
            anyhow::bail!("authenticated runtime control not attached");
        };
        let command = format!("cd -- '{PROJECT}'; printf 'SSH_%s\\n' 'PROVIDER_OWNED_OK'; pwd");
        let input = shellfox_service::native::NativeInputContext {
            captured_settlement: shellfox_host::CapturedSettlement::Unavailable,
            user_initiated: true,
            hint: false,
        };
        request(
            &client,
            NativeAction::Terminal {
                tab,
                attachment: attachment.clone(),
                command: shellfox_service::native::NativeCommand::Paste(command),
                input: Some(input.clone()),
            },
        )
        .await?;
        request(
            &client,
            NativeAction::Terminal {
                tab,
                attachment: attachment.clone(),
                command: shellfox_service::native::NativeCommand::Key(
                    shellfox_ghostty_vt::KeyEvent {
                        key: shellfox_ghostty_vt::Key::Enter,
                        ..Default::default()
                    },
                ),
                input: Some(input),
            },
        )
        .await?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let (mut marker, mut cwd) = (false, false);
        loop {
            if let NativeResponse::Frame(Some(frame)) = request(
                &client,
                NativeAction::Frame {
                    tab,
                    attachment: attachment.clone(),
                },
            )
            .await?
            {
                for row in frame
                    .frame
                    .screen
                    .cells
                    .chunks(usize::from(frame.frame.screen.cols))
                {
                    let text = row
                        .iter()
                        .map(|cell| cell.text.as_str())
                        .collect::<String>();
                    marker |= text.trim() == "SSH_PROVIDER_OWNED_OK";
                    cwd |= text.trim() == PROJECT;
                }
                // ACK only the actual consumed immutable frame, no guessed sequence/receipt.
                request(
                    &client,
                    NativeAction::AcceptFrame {
                        tab,
                        attachment: attachment.clone(),
                        frame_id: frame.frame_id,
                    },
                )
                .await?;
            }
            if marker && cwd {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "owned command output not observed"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        request(
            &client,
            NativeAction::Ssh(ssh::Request::Disconnect { connection }),
        )
        .await?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let current = status(&client, connection).await?;
            if current.state == ssh::State::Closed && current.runtime.is_none() {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "original SSH runtime cleanup deadline"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        request(
            &client,
            NativeAction::Ssh(ssh::Request::Delete { id: profile }),
        )
        .await?;
        eprintln!("authorized owned-project SSH Ready/input/output/ACK/disconnect complete; original bootstrap cleanup follows");
        Ok(())
    })?;
    fixture.success();
    Ok(())
}
