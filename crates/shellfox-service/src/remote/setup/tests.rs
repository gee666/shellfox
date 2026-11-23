//! Lexical planning is separate from the owned composed acceptance cases.
use super::*;
#[test]
fn planned_network_resources_do_not_touch_filesystem_or_create_a_proof() {
    let root =
        std::fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp"))
            .unwrap();
    let path = root
        .join(format!("planned-network-{}", uuid::Uuid::new_v4()))
        .join("node-v1");
    assert!(!path.parent().unwrap().exists());
    let planned = RemoteBootResources::planned(path.clone(), "owned-host".into()).unwrap();
    assert!(matches!(planned, RemoteBootResources::Planned { .. }));
    assert!(!path.parent().unwrap().exists());
    assert!(RemoteBootResources::planned(
        std::path::PathBuf::from("relative"),
        "owned-host".into()
    )
    .is_err());
}
