use super::*;

#[test]
fn private_renderer_rejects_host_stop_and_unscoped_system_commands() {
    for invalid in [r#"{"action":"stop"}"#, r#"{"action":"system","value":{}}"#] {
        assert!(serde_json::from_str::<DesktopRequest>(invalid).is_err());
    }
    assert!(DesktopRequest::SaveWindowState(WindowState {
        width: 99,
        height: 600,
        x: 0,
        y: 0,
        maximized: false,
        display: None,
    })
    .validate()
    .is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn attached_provider_requires_current_desktop_registration() {
    use crate::{
        desktop_launch::{DesktopLaunchReply, DesktopLaunchRequest},
        local::PrivateRequest,
        owner::ServiceOwner,
        system::{
            OwnedSystemFixture, SystemInstallPolicy, SystemNativeRequest, SystemNativeResponse,
            SystemPlatform, SystemScope,
        },
    };
    let fixture = OwnedSystemFixture::new(SystemPlatform::Linux);
    let data = fixture.root.join("attached-owner");
    std::fs::create_dir(&data).unwrap();
    let mut owner = ServiceOwner::start(
        data,
        fixture.root.join("package/Shellfox.exe"),
        shellfox_protocol::CancellationToken::new(),
        (),
    )
    .await
    .unwrap();
    let renderer = owner.client(uuid::Uuid::new_v4());
    renderer
        .install_system_native(fixture.backend(
            Vec::new(),
            SystemInstallPolicy {
                allow_user_install: true,
                allow_open: false,
            },
        ))
        .await
        .unwrap();
    let renderer_epoch = renderer.open_lifetime(uuid::Uuid::new_v4()).await.unwrap();
    let route = renderer
        .private_request_in_epoch(
            PrivateRequest::DesktopLaunch(DesktopLaunchRequest::Discover {}),
            renderer_epoch,
        )
        .await
        .unwrap();
    let DesktopLaunchReply::Route(route) = serde_json::from_value(route).unwrap() else {
        panic!("route")
    };
    let registered = renderer
        .private_request_in_epoch(
            PrivateRequest::DesktopLaunch(DesktopLaunchRequest::RegisterAttached {
                owner: route.owner,
            }),
            renderer_epoch,
        )
        .await
        .unwrap();
    let DesktopLaunchReply::Registered(token) = serde_json::from_value(registered).unwrap() else {
        panic!("registration")
    };
    let window = owner.client(uuid::Uuid::new_v4());
    let epoch = window.open_lifetime(uuid::Uuid::new_v4()).await.unwrap();
    let invoke = |token| {
        PrivateRequest::DesktopView(Box::new(DesktopRequest::Provider(
            DesktopProviderRequest::from_native(
                token,
                NativeAction::System(SystemNativeRequest::SetCli {
                    scope: SystemScope::User,
                    installed: true,
                }),
            )
            .unwrap(),
        )))
    };
    let mut forged = serde_json::to_value(token).unwrap();
    forged["nonce"] = serde_json::json!(uuid::Uuid::new_v4());
    assert!(window
        .private_request_in_epoch(invoke(serde_json::from_value(forged).unwrap()), epoch)
        .await
        .is_err());
    let result = window
        .private_request_in_epoch(invoke(token), epoch)
        .await
        .unwrap();
    let DesktopReply::System(SystemNativeResponse::Cli(state)) =
        serde_json::from_value(result).unwrap()
    else {
        panic!("CLI status")
    };
    assert!(state.installed);
    renderer
        .private_request_in_epoch(
            PrivateRequest::DesktopLaunch(DesktopLaunchRequest::UnregisterAttached { token }),
            renderer_epoch,
        )
        .await
        .unwrap();
    assert!(window
        .private_request_in_epoch(invoke(token), epoch)
        .await
        .is_err());
    window.close_lifetime(epoch).await.unwrap();
    renderer.close_lifetime(renderer_epoch).await.unwrap();
    owner.shutdown().await.unwrap();
}

#[test]
fn terminal_palette_wire_checks_the_full_256_entries() {
    use shellfox_ghostty_vt::{Rgb, TerminalColors};
    let rgb = Rgb { r: 1, g: 2, b: 3 };
    let colors = TerminalColors {
        foreground: rgb,
        background: rgb,
        cursor: None,
        palette: Some([rgb; 256]),
    };
    let mut value = serde_json::to_value(&colors).unwrap();
    assert_eq!(
        serde_json::from_value::<TerminalColors>(value.clone()).unwrap(),
        colors
    );
    value["palette"].as_array_mut().unwrap().pop();
    assert!(serde_json::from_value::<TerminalColors>(value).is_err());
}
