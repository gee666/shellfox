//! D1 public-entry coverage for the local frontend's real sibling/shared-prefix layout.
use super::*;
use crate::DirectoryRelation;
#[test]
fn public_private_entry_creates_frontend_siblings_and_shared_ui_prefix_without_host_writes() {
    let f = Fixture::new();
    let authority =
        Arc::new(FileService::approved_root(&f.path, crate::Access::ReadWrite).unwrap());
    let keeper = authority.directory_preparation_owner();
    let paths = [
        "parent/root",
        "parent/root.desktop",
        "parent/root.device",
        "parent/root.desktop/run-123-456",
        "parent/root.desktop/file-editor",
    ];
    let plans: Vec<_> = paths
        .into_iter()
        .map(|path| {
            authority
                .plan_directory(&Location { path: path.into() })
                .unwrap()
        })
        .collect();
    let validate = || {
        for plan in &plans {
            plan.revalidate().unwrap();
        }
        for (a, b) in [
            (0, 1),
            (0, 2),
            (0, 3),
            (0, 4),
            (1, 2),
            (2, 3),
            (2, 4),
            (3, 4),
        ] {
            assert_eq!(
                plans[a].relation_to(&plans[b]).unwrap(),
                DirectoryRelation::Disjoint
            );
        }
        assert_eq!(
            plans[1].relation_to(&plans[3]).unwrap(),
            DirectoryRelation::Ancestor
        );
        assert_eq!(
            plans[1].relation_to(&plans[4]).unwrap(),
            DirectoryRelation::Ancestor
        );
    };
    validate();
    let mut scopes = Vec::new();
    for index in [1, 3, 4, 2] {
        validate();
        scopes.push(plans[index].create_private_approved_root().unwrap());
        validate();
    }
    assert!(!f.path.join(paths[0]).exists());
    assert_eq!(keeper.retention().unwrap().created, 4);
    keeper.verify_retained().unwrap();
    assert!(plans[1].create_private_approved_root().is_ok());
    assert!(scopes[0]
        .create_file(&crate::CreateFile {
            path: f
                .path
                .join("parent/root.device/outside")
                .to_str()
                .unwrap()
                .into(),
            text: "outside".into()
        })
        .is_err());
    // Scope handoff is not a privacy-forever promise. No control/approval/editor activation here.
    authority.revoke().unwrap();
    assert!(scopes[0].stat(&Location { path: ".".into() }).is_err());
}
