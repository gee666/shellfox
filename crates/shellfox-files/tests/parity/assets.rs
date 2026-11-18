use super::streams::{actor, allow, capability};
use crate::support::{loc, Fixture};
use shellfox_files::*;
use std::fs;

#[test]
fn inline_reads_preserve_actual_mime_and_assets_stay_under_preview_parent() {
    let f = Fixture::new();
    fs::create_dir(f.root.join("docs")).unwrap();
    fs::create_dir(f.root.join("outside")).unwrap();
    f.write("docs/style.css", "body{}");
    f.write("docs/script.mjs", "window.demo=true");
    f.write("outside/secret.css", "secret");
    f.write("vector.svg", "<svg/>");
    assert!(
        matches!(f.service.read_file(&loc("vector.svg")).unwrap(), ReadResult::Text { mime_type, .. } if mime_type == "image/svg+xml")
    );
    for name in [
        "page.html",
        "page.xhtml",
        "README.md",
        "README.mdx",
        "README.markdown",
        "README.mdown",
        "README.mkd",
    ] {
        let base = format!("docs/{name}");
        f.write(&base, "preview");
        let result = f
            .service
            .read_asset(&AssetRequest {
                path: base.clone(),
                asset_path: "style.css".into(),
            })
            .unwrap();
        assert_eq!(result.mime_type, "text/css");
        assert_eq!(result.bytes, b"body{}");
        for path in [
            "../outside/secret.css",
            "/outside/secret.css",
            "https://example.com/a.css",
            "style.css?token",
            "style.css#fragment",
            "script.exe",
        ] {
            assert!(f
                .service
                .read_asset(&AssetRequest {
                    path: base.clone(),
                    asset_path: path.into()
                })
                .is_err());
        }
    }
    let cap = capability(&f);
    let mut registry = super::streams::ready(&f, TransferLimits::default());
    let actor = actor();
    let result = registry
        .begin_read(
            &actor,
            cap,
            &BeginRead {
                path: "docs/page.html".into(),
                asset_path: Some("script.mjs".into()),
            },
            &allow,
        )
        .unwrap();
    assert_eq!(result.mime_type, "text/javascript");
    let data = registry
        .read_chunk(
            &actor,
            &ReadChunk {
                transfer_id: result.transfer_id,
                offset: 0,
            },
            &allow,
        )
        .unwrap();
    assert_eq!(data.data, b"window.demo=true");
    f.write("docs/large.css", vec![b'x'; PREVIEW_BYTES + 1]);
    assert_eq!(
        f.service
            .read_asset(&AssetRequest {
                path: "docs/page.html".into(),
                asset_path: "large.css".into()
            })
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}

#[test]
fn image_saves_enforce_magic_mime_extension_revision_and_inline_limits() {
    let f = Fixture::new();
    for (name, mime, initial) in [
        ("image.png", "image/png", b"\x89PNG\r\n\x1a\nold".to_vec()),
        ("image.jpg", "image/jpeg", vec![255, 216, 255, 1]),
        ("image.webp", "image/webp", b"RIFF0000WEBPold".to_vec()),
    ] {
        f.write(name, &initial);
        let revision = match f.service.read_file(&loc(name)).unwrap() {
            ReadResult::Image {
                revision,
                mime_type,
                ..
            } => {
                assert_eq!(mime_type, mime);
                revision
            }
            _ => panic!("not image"),
        };
        let mut bytes = initial.clone();
        bytes.extend_from_slice(b"edited");
        let input = ImageWrite {
            path: name.into(),
            revision: revision.clone(),
            mime_type: mime.into(),
            bytes: bytes.clone(),
        };
        f.service.write_image(&input).unwrap();
        assert_eq!(f.read(name), bytes);
        assert_eq!(
            f.service.write_image(&input).unwrap_err().code,
            ErrorCode::FileConflict
        );
        let revision = match f.service.read_file(&loc(name)).unwrap() {
            ReadResult::Image { revision, .. } => revision,
            _ => panic!("not image"),
        };
        assert_eq!(
            f.service
                .write_image(&ImageWrite {
                    path: name.into(),
                    revision,
                    mime_type: mime.into(),
                    bytes: vec![b'x'; INLINE_IMAGE_WRITE_BYTES + 1]
                })
                .unwrap_err()
                .code,
            ErrorCode::Validation
        );
    }
    f.write("wrong.jpg", b"\x89PNG\r\n\x1a\nold");
    let revision = match f.service.read_file(&loc("wrong.jpg")).unwrap() {
        ReadResult::Image { revision, .. } => revision,
        _ => panic!("not image"),
    };
    assert_eq!(
        f.service
            .write_image(&ImageWrite {
                path: "wrong.jpg".into(),
                revision,
                mime_type: "image/png".into(),
                bytes: b"\x89PNG\r\n\x1a\nnew".to_vec()
            })
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    assert_eq!(
        fs::read_dir(&f.root)
            .unwrap()
            .filter(|entry| entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".shellfox"))
            .count(),
        0
    );
}
