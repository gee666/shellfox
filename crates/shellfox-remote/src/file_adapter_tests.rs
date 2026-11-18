use crate::{assets, directories, file_ops, test_support, ViewPermission};
use shellfox_protocol::{self as wire, Validate};
#[test]
fn inline_mime_images_assets_and_directory_prompts_reuse_approved_root_service() {
    let (scratch, actor) = test_support::actor(ViewPermission::Write);
    let home = scratch.path().to_str().unwrap();
    let html = scratch.path().join("index.html");
    let css = scratch.path().join("style.css");
    std::fs::write(&html, b"<p>fixture</p>").unwrap();
    std::fs::write(&css, b"p { color: red; }").unwrap();
    let result = actor
        .with_authorized(None, || file_ops::read(&actor, html.to_str().unwrap()))
        .unwrap();
    assert_eq!(result["mimeType"], "text/html");
    let asset = wire::ReadFileAsset {
        session_id: None,
        tab_id: None,
        path: html.to_str().unwrap().into(),
        asset_path: "style.css".into(),
    };
    let result = actor
        .with_authorized(None, || assets::read(&actor, &asset))
        .unwrap();
    serde_json::from_value::<wire::FileAsset>(result.clone())
        .unwrap()
        .validate()
        .unwrap();
    assert_eq!(result["mimeType"], "text/css");
    let png = wire::decode_chunk("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jY9kAAAAASUVORK5CYII=").unwrap();
    let path = scratch.path().join("pixel.png");
    std::fs::write(&path, &png).unwrap();
    let image = actor
        .with_authorized(None, || file_ops::read(&actor, path.to_str().unwrap()))
        .unwrap();
    assert_eq!(image["kind"], "image");
    assert_eq!(image["mimeType"], "image/png");
    let write = wire::WriteImageFile {
        session_id: None,
        tab_id: None,
        path: path.to_str().unwrap().into(),
        revision: image["revision"].as_str().unwrap().into(),
        data_url: image["dataUrl"].as_str().unwrap().into(),
    };
    let result = actor
        .with_authorized(
            Some(&wire::RemoteCommand::WriteImageFile(write.clone())),
            || assets::write(&actor, &write),
        )
        .unwrap();
    assert_eq!(result["size"], png.len());
    let create = wire::CreateDirectory {
        session_id: None,
        tab_id: None,
        path: "new dir".into(),
    };
    actor
        .with_authorized(
            Some(&wire::RemoteCommand::CreateDirectory(create.clone())),
            || directories::create(&actor, &create, Some(home)),
        )
        .unwrap();
    assert!(scratch.path().join("new dir").is_dir());
    let resolved = actor
        .with_authorized(None, || {
            directories::resolve(
                &actor,
                &wire::ResolveDirectory {
                    path: "~/new dir".into(),
                },
                Some(home),
            )
        })
        .unwrap();
    assert_eq!(
        resolved["cwd"],
        scratch.path().join("new dir").to_str().unwrap()
    );
    let matches = actor
        .with_authorized(None, || {
            directories::complete(
                &actor,
                &wire::CompleteDirectory { path: "ne".into() },
                Some(home),
            )
        })
        .unwrap();
    assert_eq!(matches["matches"].as_array().unwrap().len(), 1);
    let escaped = wire::ReadFileAsset {
        asset_path: "../outside.css".into(),
        ..asset
    };
    assert!(actor
        .with_authorized(None, || assets::read(&actor, &escaped))
        .is_err());
}
#[test]
fn legacy_readonly_approved_device_has_fixed_control_full_account_but_missing_home_is_explicit() {
    let (scratch, actor) = test_support::actor(ViewPermission::ReadOnly);
    let input = wire::CreateDirectory {
        session_id: None,
        tab_id: None,
        path: scratch.path().join("new").to_str().unwrap().into(),
    };
    assert_eq!(actor.permission, ViewPermission::Write);
    assert_eq!(
        actor.file_capabilities()[0].capability_kind(),
        shellfox_files::CapabilityKind::ApprovedFullAccount
    );
    actor
        .with_authorized(None, || directories::create(&actor, &input, None))
        .unwrap();
    assert!(scratch.path().join("new").is_dir());
    assert_eq!(
        directories::resolve(
            &actor,
            &wire::ResolveDirectory {
                path: "relative".into()
            },
            None
        )
        .unwrap_err()
        .code,
        wire::ErrorCode::Unsupported
    );
}
