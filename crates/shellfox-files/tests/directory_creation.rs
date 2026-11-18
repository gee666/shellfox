#[path = "support/mod.rs"]
pub mod support;
use shellfox_files::{
    Access, CapabilityKind, CreateFile, DirectoryPlan, DirectoryRelation as R, ErrorCode,
    FileService, Location,
};
use std::{path::Path, sync::Arc};
use support::Fixture;
fn location(path: &Path) -> Location {
    Location {
        path: path.to_str().unwrap().into(),
    }
}
fn topology(plans: &[DirectoryPlan]) {
    for plan in plans {
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
        assert_eq!(plans[a].relation_to(&plans[b]).unwrap(), R::Disjoint);
    }
    assert_eq!(plans[1].relation_to(&plans[3]).unwrap(), R::Ancestor);
    assert_eq!(plans[1].relation_to(&plans[4]).unwrap(), R::Ancestor);
}
#[test]
fn normal_fresh_defaults_create_without_host_writes_and_all_retained_plans_stay_bound() {
    let fixture = Fixture::new();
    let authority = Arc::new(FileService::approved_root(&fixture.root, Access::ReadWrite).unwrap());
    let names = [
        "root",
        "root.desktop",
        "root.device",
        "root.desktop/run-123-456",
        "root.desktop/file-editor",
    ];
    let plans: Vec<_> = names
        .iter()
        .map(|name| {
            authority
                .plan_directory(&location(&fixture.root.join(name)))
                .unwrap()
        })
        .collect();
    topology(&plans);
    let mut roots = Vec::new();
    for index in [1, 3, 4, 2] {
        topology(&plans);
        let root = plans[index].create_approved_root().unwrap();
        assert_eq!(root.capability_kind(), CapabilityKind::ApprovedRoot);
        roots.push(root);
        topology(&plans);
    }
    assert!(!plans[0].configured_path().exists());
    assert!(
        plans[1].create_approved_root().is_ok(),
        "same capability may reopen its exact owned planned root"
    );
    assert!(roots[0]
        .create_file(&CreateFile {
            path: fixture
                .root
                .join("root.device/outside.txt")
                .to_str()
                .unwrap()
                .into(),
            text: "outside".into()
        })
        .is_err());
}
#[test]
fn readonly_and_revoked_original_capabilities_cannot_create_or_upgrade() {
    let fixture = Fixture::new();
    let readonly = Arc::new(FileService::approved_root(&fixture.root, Access::ReadOnly).unwrap());
    let denied = readonly
        .plan_directory(&location(&fixture.root.join("denied")))
        .unwrap();
    assert_eq!(
        denied.create_approved_root().err().unwrap().error().code,
        ErrorCode::AccessDenied
    );
    assert!(!fixture.root.join("denied").exists());
    let authority = Arc::new(FileService::approved_root(&fixture.root, Access::ReadWrite).unwrap());
    let a = authority
        .plan_directory(&location(&fixture.root.join("a")))
        .unwrap();
    let b = authority
        .plan_directory(&location(&fixture.root.join("b")))
        .unwrap();
    let child = a.create_approved_root().unwrap();
    let sibling = b.create_approved_root().unwrap();
    child.revoke().unwrap();
    sibling
        .create_file(&CreateFile {
            path: fixture
                .root
                .join("b/still-owned.txt")
                .to_str()
                .unwrap()
                .into(),
            text: "owned".into(),
        })
        .unwrap();
    authority.revoke().unwrap();
    assert_eq!(
        sibling
            .list(&location(&fixture.root.join("b")))
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    assert!(b.create_approved_root().is_err());
}
#[test]
fn external_plain_directory_creation_is_not_silently_adopted_even_with_write_authority() {
    let fixture = Fixture::new();
    let authority = Arc::new(FileService::approved_root(&fixture.root, Access::ReadWrite).unwrap());
    let plan = authority
        .plan_directory(&location(&fixture.root.join("external/child")))
        .unwrap();
    std::fs::create_dir(fixture.root.join("external")).unwrap();
    assert_eq!(
        plan.create_approved_root().err().unwrap().error().code,
        ErrorCode::FileConflict
    );
    assert!(!fixture.root.join("external/child").exists());
}
#[test]
fn ambiguous_case_plans_reject_before_any_create_and_cannot_adopt_a_later_alias_prefix() {
    let fixture = Fixture::new();
    let authority = Arc::new(FileService::approved_root(&fixture.root, Access::ReadWrite).unwrap());
    let first = authority
        .plan_directory(&location(&fixture.root.join("future")))
        .unwrap();
    let alias = authority
        .plan_directory(&location(&fixture.root.join("FUTURE/child")))
        .unwrap();
    assert_eq!(first.relation_to(&alias).unwrap(), R::Unproven);
    assert!(!first.configured_path().exists());
    assert!(!alias.configured_path().exists());
    let _root = first.create_approved_root().unwrap();
    if fixture.root.join("FUTURE").is_dir() {
        assert_eq!(
            alias.create_approved_root().err().unwrap().error().code,
            ErrorCode::FileConflict
        );
        assert!(!fixture.root.join("future/child").exists());
    }
}
