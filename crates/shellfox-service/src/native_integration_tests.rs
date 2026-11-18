//! Opt-in real PTY integration. Never uses a running user window or installed data.
#![cfg(windows)]
use super::*;
use anyhow::{Context, Result};
use shellfox_core::{CreateSession, ProfileId};
use shellfox_ghostty_vt::{Key, KeyEvent};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires an explicit separate supervisor artifact in SHELLFOX_NATIVE_TEST_SUPERVISOR"]
fn native_and_readonly_client_share_one_real_runtime_and_detach_does_not_kill() {
    fn marker(frame: &ScreenFrame) -> bool {
        frame
            .screen
            .cells
            .chunks(usize::from(frame.screen.cols))
            .any(|row| {
                row.iter()
                    .map(|c| c.text.as_str())
                    .collect::<String>()
                    .trim()
                    == "NATIVE_SINGLE_OWNER_OK"
            })
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let data = tempfile::Builder::new()
        .prefix("native-owned-pty-")
        .tempdir_in(root)
        .unwrap();
    let supervisor = PathBuf::from(
        std::env::var_os("SHELLFOX_NATIVE_TEST_SUPERVISOR").expect("explicit test artifact"),
    );
    let mut service = crate::Service::open_owned(data.path(), &supervisor).unwrap();
    let result = (|| -> Result<()> {
        let input = CreateSession {
            request_id: SessionId::new(),
            title: Some("native owned fixture".into()),
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: ProfileId::new("cmd")?,
        };
        let NativeResponse::Snapshot(snapshot) =
            service.execute_native(NativeAction::Metadata(HostRequest::CreateSession(input)), 1)?
        else {
            anyhow::bail!("snapshot")
        };
        let tab = snapshot.sessions[0].tabs[0].id;
        let input_context = NativeInputContext {
            captured_settlement: shellfox_host::CapturedSettlement::Captured(
                snapshot.sessions[0].settled_at.clone(),
            ),
            user_initiated: true,
            hint: true,
        };
        let NativeResponse::Attached(control) = service.execute_native(
            NativeAction::Attach {
                tab,
                takeover: false,
            },
            1,
        )?
        else {
            anyhow::bail!("control attachment")
        };
        let NativeResponse::Observed(observer) =
            service.execute_native(NativeAction::Observe { tab }, 2)?
        else {
            anyhow::bail!("observer attachment")
        };
        assert_eq!(service.active_terminals(), 1);
        assert!(service
            .execute_native(
                NativeAction::Terminal {
                    tab,
                    attachment: control.clone(),
                    command: NativeCommand::Paste("unauthorized".into()),
                    input: Some(input_context.clone()),
                },
                2
            )
            .is_err());
        service.execute_native(
            NativeAction::Terminal {
                tab,
                attachment: control.clone(),
                command: NativeCommand::Paste("echo NATIVE_SINGLE_OWNER_OK".into()),
                input: Some(input_context.clone()),
            },
            1,
        )?;
        service.execute_native(
            NativeAction::Terminal {
                tab,
                attachment: control.clone(),
                command: NativeCommand::Key(KeyEvent {
                    key: Key::Enter,
                    ..Default::default()
                }),
                input: Some(input_context.clone()),
            },
            1,
        )?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut seen = false;
        let mut observer_seen = false;
        let mut confirmed_credit = false;
        while Instant::now() < deadline && !(seen && observer_seen && confirmed_credit) {
            service.pump();
            if let NativeResponse::Frame(Some(frame)) = service.execute_native(
                NativeAction::Frame {
                    tab,
                    attachment: control.clone(),
                },
                1,
            )? {
                seen |= marker(&frame.frame);
                let NativeResponse::Accepted { sequence } = service.execute_native(
                    NativeAction::AcceptFrame {
                        tab,
                        attachment: control.clone(),
                        frame_id: frame.frame_id,
                    },
                    1,
                )?
                else {
                    anyhow::bail!("accept")
                };
                confirmed_credit |= sequence > 0;
                if let NativeResponse::Frame(Some(readonly)) = service.execute_native(
                    NativeAction::ObserverFrame {
                        tab,
                        attachment: observer.clone(),
                    },
                    2,
                )? {
                    if frame.frame_id == readonly.frame_id {
                        assert!(Arc::ptr_eq(&frame.frame, &readonly.frame));
                    }
                    observer_seen |= marker(&readonly.frame);
                    service.execute_native(
                        NativeAction::AcceptObserverFrame {
                            tab,
                            attachment: observer.clone(),
                            frame_id: readonly.frame_id,
                        },
                        2,
                    )?;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        anyhow::ensure!(
            seen && observer_seen && confirmed_credit,
            "Native/observer frame acceptance timed out."
        );
        let NativeResponse::Frame(Some(before)) = service.execute_native(
            NativeAction::ObserverFrame {
                tab,
                attachment: observer.clone(),
            },
            2,
        )?
        else {
            anyhow::bail!("observer frame")
        };
        let row = before
            .frame
            .screen
            .cells
            .chunks(usize::from(before.frame.screen.cols))
            .position(|row| {
                row.iter()
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
                    .trim()
                    == "NATIVE_SINGLE_OWNER_OK"
            })
            .context("marker row")? as u16;
        service.execute_native(
            NativeAction::Presentation {
                tab,
                attachment: NativeViewAttachment::Observer(observer.clone()),
                action: NativePresentation::Selection {
                    selection: Some(shellfox_ghostty_vt::Selection::Range {
                        start: shellfox_ghostty_vt::ViewportPoint { x: 0, y: row },
                        end: shellfox_ghostty_vt::ViewportPoint { x: 5, y: row },
                        rectangle: false,
                    }),
                },
            },
            2,
        )?;
        let copied = service.execute_native(
            NativeAction::Presentation {
                tab,
                attachment: NativeViewAttachment::Observer(observer.clone()),
                action: NativePresentation::Copy,
            },
            2,
        )?;
        anyhow::ensure!(
            matches!(copied, NativeResponse::Terminal(shellfox_runtime::Response::Text(Some(ref text))) if text == "NATIVE"),
            "private observer copy mismatch"
        );
        let NativeResponse::Frame(Some(selected)) = service.execute_native(
            NativeAction::ObserverFrame {
                tab,
                attachment: observer.clone(),
            },
            2,
        )?
        else {
            anyhow::bail!("selected observer frame")
        };
        anyhow::ensure!(
            selected.presentation.version > before.presentation.version
                && selected.frame_id != before.frame_id
        );
        let NativeResponse::Frame(Some(controller_frame)) = service.execute_native(
            NativeAction::Frame {
                tab,
                attachment: control.clone(),
            },
            1,
        )?
        else {
            anyhow::bail!("controller frame")
        };
        anyhow::ensure!(
            !controller_frame
                .frame
                .screen
                .cells
                .iter()
                .any(|cell| cell.selected),
            "observer changed controller selection"
        );
        service.execute_native(
            NativeAction::Detach {
                tab,
                attachment: control.clone(),
            },
            1,
        )?;
        assert_eq!(service.active_terminals(), 1);
        let replacement = service.attach(tab, 3, false)?; // CLI attaches the same registry/PTY.
        assert_eq!(replacement.generation, control.generation);
        service.execute_native(
            NativeAction::Detach {
                tab,
                attachment: control.clone(),
            },
            1,
        )?;
        // Retiring an already-revoked GUI token must not detach the CLI lease.
        service.resize(tab, &replacement, 80, 24)?;
        assert!(service
            .execute_native(
                NativeAction::Frame {
                    tab,
                    attachment: control
                },
                1
            )
            .is_err());
        service.execute_native(
            NativeAction::Unobserve {
                tab,
                attachment: observer,
            },
            2,
        )?;
        service.close_tab(tab)?;
        assert_eq!(service.active_terminals(), 0);
        Ok(())
    })();
    service
        .shutdown()
        .context("owned native test cleanup")
        .unwrap();
    result.unwrap();
}
