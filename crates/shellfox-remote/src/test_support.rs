use crate::{Actor, Approval, ApprovalControl, ViewPermission};
use shellfox_protocol as wire;
use shellfox_transport::incoming::PeerIdentity;
use std::path::Path;

pub(crate) fn actor(permission: ViewPermission) -> (tempfile::TempDir, Actor) {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&parent).unwrap();
    let parent = std::fs::canonicalize(parent).unwrap();
    #[cfg(windows)]
    let parent = std::path::PathBuf::from(
        parent
            .to_str()
            .unwrap()
            .strip_prefix(r"\\?\")
            .unwrap_or(parent.to_str().unwrap()),
    );
    let scratch = tempfile::Builder::new()
        .prefix("remote-transfer-")
        .tempdir_in(parent)
        .unwrap();
    let control = ApprovalControl::default();
    control
        .install(Approval {
            peer_id: "n-transfer".into(),
            hostname: "offline".into(),
            approved_at_millis: 1,
            permission,
            file_roots: vec![scratch.path().to_owned()],
        })
        .unwrap();
    let mut actor = control
        .authorize(
            &PeerIdentity {
                id: "n-transfer".into(),
                hostname: "offline".into(),
            },
            wire::ClientId::new_v4(),
            1,
        )
        .unwrap();
    actor.view_epoch = 1;
    (scratch, actor)
}
