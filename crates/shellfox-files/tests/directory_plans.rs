#[cfg(windows)]
#[path = "support/directory_alias.rs"]
mod native_alias;
#[path = "support/mod.rs"]
pub mod support;
use shellfox_files::{Access, DirectoryRelation as R, ErrorCode, FileService, Location};
use std::{path::Path, sync::Arc};
use support::Fixture;
fn location(path: &Path) -> Location {
    Location {
        path: path.to_str().unwrap().into(),
    }
}
#[test]
fn existing_native_identities_prove_same_ancestors_and_distinct_siblings() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.root.join("parent/child")).unwrap();
    std::fs::create_dir(fixture.root.join("sibling")).unwrap();
    let cap = Arc::new(FileService::local_desktop(Access::ReadOnly));
    let parent = cap
        .plan_directory(&location(&fixture.root.join("parent")))
        .unwrap();
    let child = cap
        .plan_directory(&location(&fixture.root.join("parent/child")))
        .unwrap();
    let sibling = cap
        .plan_directory(&location(&fixture.root.join("sibling")))
        .unwrap();
    assert_eq!(parent.relation_to(&parent).unwrap(), R::Same);
    assert_eq!(parent.relation_to(&child).unwrap(), R::Ancestor);
    assert_eq!(child.relation_to(&parent).unwrap(), R::Descendant);
    assert_eq!(parent.relation_to(&sibling).unwrap(), R::Disjoint);
    #[cfg(any(windows, target_os = "macos"))]
    {
        let alternate = fixture.root.join("PARENT");
        if alternate.is_dir() {
            let alias = cap.plan_directory(&location(&alternate)).unwrap();
            assert_eq!(parent.relation_to(&alias).unwrap(), R::Same);
        }
    }
}
#[test]
fn fresh_default_siblings_and_ui_descendants_have_explicit_native_name_proof() {
    let fixture = Fixture::new();
    let cap = Arc::new(FileService::local_desktop(Access::ReadOnly));
    let host = cap
        .plan_directory(&location(&fixture.root.join("root")))
        .unwrap();
    let ui = cap
        .plan_directory(&location(&fixture.root.join("root.desktop")))
        .unwrap();
    let device = cap
        .plan_directory(&location(&fixture.root.join("root.device")))
        .unwrap();
    let run = cap
        .plan_directory(&location(&fixture.root.join("root.desktop/run-123-456")))
        .unwrap();
    let editor = cap
        .plan_directory(&location(&fixture.root.join("root.desktop/file-editor")))
        .unwrap();
    assert_eq!(host.relation_to(&ui).unwrap(), R::Disjoint);
    assert_eq!(host.relation_to(&device).unwrap(), R::Disjoint);
    assert_eq!(ui.relation_to(&device).unwrap(), R::Disjoint);
    assert_eq!(ui.relation_to(&run).unwrap(), R::Ancestor);
    assert_eq!(ui.relation_to(&editor).unwrap(), R::Ancestor);
    assert_eq!(run.relation_to(&editor).unwrap(), R::Disjoint);
    for plan in [&host, &ui, &device, &run, &editor] {
        plan.revalidate().unwrap();
        assert!(!plan.configured_path().exists());
    }
}
#[test]
fn unresolved_case_unicode_and_alternate_name_ambiguity_is_not_lexical_disjointness() {
    let fixture = Fixture::new();
    let cap = Arc::new(FileService::local_desktop(Access::ReadOnly));
    for (a, b) in [
        ("missing", "MISSING"),
        ("long-root", "LONG-R~1"),
        ("unicode-é", "unicode-e\u{301}"),
    ] {
        let a = cap
            .plan_directory(&location(&fixture.root.join(a)))
            .unwrap();
        let b = cap
            .plan_directory(&location(&fixture.root.join(b)))
            .unwrap();
        assert_eq!(a.relation_to(&b).unwrap(), R::Unproven);
        assert!(!a.configured_path().exists());
        assert!(!b.configured_path().exists());
    }
}
#[test]
fn revalidation_never_adopts_new_plain_directories_or_revoked_and_out_of_scope_roots() {
    let fixture = Fixture::new();
    let cap = Arc::new(FileService::approved_root(&fixture.root, Access::ReadOnly).unwrap());
    let planned = cap
        .plan_directory(&location(&fixture.root.join("missing/child")))
        .unwrap();
    std::fs::create_dir(fixture.root.join("missing")).unwrap();
    assert_eq!(
        planned.revalidate().unwrap_err().code,
        ErrorCode::FileConflict
    );
    assert!(cap
        .plan_directory(&location(&fixture.root.with_file_name("outside-plan")))
        .is_err());
    let stable = cap.plan_directory(&location(&fixture.root)).unwrap();
    cap.revoke().unwrap();
    assert_eq!(
        stable.revalidate().unwrap_err().code,
        ErrorCode::AccessDenied
    );
    let cap = Arc::new(FileService::local_desktop(Access::ReadOnly));
    let mut deep = fixture.root.join("not-created");
    for _ in 0..257 {
        deep.push("x");
    }
    assert!(cap.plan_directory(&location(&deep)).is_err());
    assert!(!fixture.root.join("not-created").exists());
}
