use super::*;
use crate::{
    CreateFile, ImageWrite, Location, ReadResult, WriteText, INLINE_IMAGE_WRITE_BYTES, TEXT_BYTES,
};
#[test]
fn v2_bounded_inline_save_keeps_cas_and_does_not_become_a_large_editor_fallback() {
    let f = Fixture::new();
    f.cap
        .create_file(&CreateFile {
            path: "inline.txt".into(),
            text: "old".into(),
        })
        .unwrap();
    let old = f
        .cap
        .read_text(&Location {
            path: "inline.txt".into(),
        })
        .unwrap();
    let request = WriteText {
        path: old.path,
        revision: old.revision,
        text: "new".into(),
    };
    f.cap.write_text(&request).unwrap();
    assert_eq!(fs::read(f.root.join("user/inline.txt")).unwrap(), b"new");
    assert_eq!(
        f.cap.write_text(&request).unwrap_err().code,
        crate::ErrorCode::FileConflict
    );
    let current = f
        .cap
        .read_text(&Location {
            path: "inline.txt".into(),
        })
        .unwrap();
    assert_eq!(
        f.cap
            .write_text(&WriteText {
                path: current.path,
                revision: current.revision,
                text: "x".repeat(TEXT_BYTES + 1)
            })
            .unwrap_err()
            .code,
        crate::ErrorCode::Validation
    );
    assert_eq!(fs::read_dir(f.root.join("user")).unwrap().count(), 1);
}
#[test]
fn v2_bounded_image_save_uses_retained_destination_semantics_and_preserves_limits() {
    let f = Fixture::new();
    fs::write(f.root.join("user/image.png"), b"\x89PNG\r\n\x1a\nold").unwrap();
    let ReadResult::Image { revision, .. } = f
        .cap
        .read_file(&Location {
            path: "image.png".into(),
        })
        .unwrap()
    else {
        panic!("expected image");
    };
    let input = ImageWrite {
        path: "image.png".into(),
        revision,
        mime_type: "image/png".into(),
        bytes: b"\x89PNG\r\n\x1a\nnew".to_vec(),
    };
    f.cap.write_image(&input).unwrap();
    assert_eq!(
        fs::read(f.root.join("user/image.png")).unwrap(),
        input.bytes
    );
    assert_eq!(
        f.cap.write_image(&input).unwrap_err().code,
        crate::ErrorCode::FileConflict
    );
    let ReadResult::Image { revision, .. } = f
        .cap
        .read_file(&Location {
            path: "image.png".into(),
        })
        .unwrap()
    else {
        panic!("expected image");
    };
    assert_eq!(
        f.cap
            .write_image(&ImageWrite {
                path: "image.png".into(),
                revision,
                mime_type: "image/png".into(),
                bytes: vec![b'x'; INLINE_IMAGE_WRITE_BYTES + 1]
            })
            .unwrap_err()
            .code,
        crate::ErrorCode::Validation
    );
    assert_eq!(fs::read_dir(f.root.join("user")).unwrap().count(), 1);
}
