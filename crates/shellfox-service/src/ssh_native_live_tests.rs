use super::*;
struct AuthGate(PathBuf);
impl Drop for AuthGate {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
#[test]
#[ignore = "requires explicit owned fake program and separate trusted supervisor; never real SSH"]
fn native_same_service_runtime_keeps_live_activity_without_view_credit() {
    let program = std::env::var_os("SHELLFOX_SSH_NATIVE_TEST_PROGRAM")
        .map(PathBuf::from)
        .expect("owned fake program");
    let supervisor = std::env::var_os("SHELLFOX_SSH_NATIVE_TEST_SUPERVISOR")
        .map(PathBuf::from)
        .expect("owned supervisor");
    let gate = AuthGate(program.parent().unwrap().join("auth-pending"));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&gate.0)
        .unwrap();
    let askpass = std::env::var_os("SHELLFOX_SSH_NATIVE_TEST_ASKPASS_PROGRAM")
        .map(PathBuf::from)
        .expect("explicit configured role-approved askpass helper");
    let mut f = Fixture::with_askpass(program, supervisor, askpass);
    let profile = f.profile();
    let crate::native::NativeResponse::SshOpened(opened) = f
        .service
        .execute_native(
            crate::native::NativeAction::SshOpen(OpenProfile {
                request_id: SessionId::new(),
                profile,
                cols: 80,
                rows: 24,
            }),
            7,
        )
        .unwrap()
    else {
        panic!("opened profile")
    };
    let id = opened.connection.connection;
    f.drive(id, |s| matches!(s, State::AwaitingHost(_)));
    let State::AwaitingHost(challenge) = f.state(id) else {
        panic!("challenge")
    };
    f.service
        .handle_ssh_native(
            Request::ConfirmHost {
                connection: id,
                challenge: challenge.challenge,
                decision: TrustDecision::TrustAndSave,
            },
            7,
        )
        .unwrap();
    f.drive(id, |s| *s == State::Authenticating);
    assert_eq!(f.service.active_terminals(), 1);
    let runtime = f
        .status(id)
        .runtime
        .expect("owner-validated current presentation identity");
    assert_eq!(runtime.tab_id, opened.connection.tab);
    assert_eq!(
        f.service.terminals[&runtime.tab_id].runtime.identity(),
        runtime
    );
    assert!(f
        .service
        .check_ssh_control_admission(7, runtime.tab_id, runtime)
        .is_err());
    assert!(f
        .service
        .check_ssh_control_admission(8, runtime.tab_id, runtime)
        .is_err());
    let crate::native::NativeResponse::Observed(observer) = f
        .service
        .execute_native(
            crate::native::NativeAction::Observe {
                tab: runtime.tab_id,
            },
            8,
        )
        .unwrap()
    else {
        panic!("readonly observer")
    };
    assert_eq!(
        observer.generation, runtime.generation,
        "Root must reject a raced mismatched observer before publication"
    );
    let original = f
        .service
        .ssh
        .installed
        .as_mut()
        .unwrap()
        .runtime
        .started
        .insert(
            runtime.tab_id,
            shellfox_runtime::Identity {
                generation: shellfox_core::Generation::new(),
                ..runtime
            },
        )
        .unwrap();
    assert!(
        f.status(id).runtime.is_none(),
        "stale manager/registry identity must not be exposed"
    );
    f.service
        .ssh
        .installed
        .as_mut()
        .unwrap()
        .runtime
        .started
        .insert(runtime.tab_id, original);
    assert!(
        f.service.attach(runtime.tab_id, 8, true).is_err(),
        "pending SSH must not acquire a generic Claim"
    );
    f.service
        .execute_native(
            crate::native::NativeAction::Unobserve {
                tab: runtime.tab_id,
                attachment: observer,
            },
            8,
        )
        .unwrap();
    std::fs::remove_file(&gate.0).unwrap();
    f.drive(id, |s| matches!(s, State::AwaitingPrompt(_)));
    let State::AwaitingPrompt(prompt) = f.state(id) else {
        panic!("owned callback prompt")
    };
    assert_eq!(prompt.label, "--shellfox-pty-supervisor");
    f.service
        .handle_ssh_native(
            Request::AnswerPrompt {
                connection: id,
                prompt: prompt.prompt,
                answer: Some(Secret::new("synthetic-native-answer".into()).unwrap()),
            },
            7,
        )
        .unwrap();
    f.drive(id, |s| *s == State::Ready);
    assert!(f
        .service
        .check_ssh_control_admission(8, runtime.tab_id, runtime)
        .is_ok());
    let mut stale = runtime;
    stale.generation = shellfox_core::Generation::new();
    assert!(f
        .service
        .check_ssh_control_admission(8, runtime.tab_id, stale)
        .is_err());
    let sink = f.service.activity.owner.live_output_sink(runtime).unwrap();
    assert!(sink.status().1);
    assert_eq!(
        f.service.terminals[&runtime.tab_id]
            .runtime
            .live_output_status(),
        shellfox_runtime::LiveOutputStatus::Active
    );
    assert!(f.service.host.session_creation_facts().unwrap().is_empty());
    f.service.detach_ssh_native(7);
    f.service.pump();
    assert_eq!(f.service.active_terminals(), 1);
    f.service.admit_ssh_native(8).unwrap();
    f.service
        .handle_ssh_native(Request::AdoptReady { connection: id }, 8)
        .unwrap();
    let settled = f
        .service
        .host
        .set_session_settled(runtime.session_id, true)
        .unwrap();
    let stamp = settled
        .sessions
        .iter()
        .find(|s| s.id == runtime.session_id)
        .unwrap()
        .settled_at
        .clone();
    let crate::native::NativeResponse::Attached(attachment) = f
        .service
        .execute_native(
            crate::native::NativeAction::Attach {
                tab: runtime.tab_id,
                takeover: false,
            },
            8,
        )
        .unwrap()
    else {
        panic!("attachment")
    };
    f.service
        .execute_native(
            crate::native::NativeAction::Terminal {
                tab: runtime.tab_id,
                attachment,
                command: crate::native::NativeCommand::Paste("x".into()),
                input: Some(crate::native::NativeInputContext {
                    captured_settlement: shellfox_host::CapturedSettlement::Captured(stamp),
                    user_initiated: true,
                    hint: false,
                }),
            },
            8,
        )
        .unwrap();
    assert!(f
        .service
        .host
        .snapshot()
        .unwrap()
        .sessions
        .iter()
        .find(|s| s.id == runtime.session_id)
        .unwrap()
        .settled_at
        .is_none());
    f.service.revoke_ssh_native(8);
    assert!(f
        .service
        .check_ssh_control_admission(8, runtime.tab_id, runtime)
        .is_err());
    f.stop();
    assert_eq!(f.service.active_terminals(), 0);
}
