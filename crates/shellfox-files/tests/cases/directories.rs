use crate::support::{loc, Fixture};
use shellfox_files::*;
use std::fs;

#[test]
fn directory_prompts_are_scoped_bounded_and_use_explicit_authoritative_context() {
    let f = Fixture::new();
    fs::create_dir_all(f.root.join("work space/alpha")).unwrap();
    fs::create_dir_all(f.root.join("work space/beta")).unwrap();
    let context = FileContext {
        cwd: f.root.join("work space").to_str().unwrap().into(),
        home: Some(f.root.to_str().unwrap().into()),
    };
    let result = f
        .service
        .resolve_directory_with_context(&loc("'~/work space'"), Some(&context))
        .unwrap();
    assert_eq!(result.cwd, f.root.join("work space").to_str().unwrap());
    assert_eq!(
        f.service
            .resolve_location(&loc("alpha"), &context)
            .unwrap()
            .path,
        f.root.join("work space").join("alpha").to_str().unwrap()
    );
    assert_eq!(
        f.service
            .complete_directory_with_context(&loc("~/work space/a"), Some(&context))
            .unwrap()
            .matches,
        ["~/work space/alpha/".to_owned()]
    );
    f.service
        .create_directory_with_context(&loc("'~/created/nested'"), Some(&context))
        .unwrap();
    assert!(f.root.join("created/nested").is_dir());
    f.write("regular", "file");
    assert_eq!(
        f.service
            .resolve_directory(&loc("regular/child"))
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    assert_eq!(
        f.service
            .create_directory_with_context(&loc("regular/child"), None)
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    let matches = f.service.complete_directory(&loc("work")).unwrap().matches;
    assert_eq!(
        matches,
        [format!("work space{}", std::path::MAIN_SEPARATOR)]
    );
    assert!(f.service.resolve_directory(&loc("~")).is_err());
    assert!(f
        .service
        .resolve_location(&loc("../outside"), &context)
        .is_err());
    let outside = Fixture::new();
    let bad = FileContext {
        cwd: outside.root.to_str().unwrap().into(),
        home: Some(outside.root.to_str().unwrap().into()),
    };
    assert_eq!(
        f.service
            .resolve_location(&loc("file"), &bad)
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    for index in 0..DIRECTORY_COMPLETION_LIMIT + 5 {
        fs::create_dir(f.root.join(format!("folder-{index:03}"))).unwrap();
    }
    let result = f.service.complete_directory(&loc("folder")).unwrap();
    assert_eq!(result.matches.len(), DIRECTORY_COMPLETION_LIMIT);
    assert!(result.matches.iter().map(String::len).sum::<usize>() <= DIRECTORY_COMPLETION_BYTES);
    assert!(result.matches[0].starts_with("folder-000"));
    for input in ["a\0b", "C:relative", "http://example.com", "a/../b"] {
        assert!(f.service.complete_directory(&loc(input)).is_err());
    }
}
