#[test]
fn privileged_discriminators_precede_normal_startup_and_logging() {
    let source = include_str!("main.rs");
    let dispatch = source
        .find("shellfox_service::dispatch_supervisor_entry()")
        .unwrap();
    let normal = source.find("match ssh::dispatch").unwrap();
    assert!(dispatch < normal);
    assert!(source.contains("i32::from_ne_bytes(code.to_ne_bytes())"));
    assert!(
        !source.contains("shellfox_terminal::supervisor_entry()"),
        "mode policy belongs only to shared dispatcher"
    );
}
