#[test]
fn privileged_discriminators_precede_normal_startup_and_logging() {
    let source = include_str!("main.rs");
    let dispatch = source
        .find("shellfox_service::dispatch_supervisor_entry()")
        .unwrap();
    // The desktop host process is this binary too; its shells re-enter as supervisors.
    let desktop_host = source.find("desktop_host::run(").unwrap();
    let launcher = source.find("parent_watch::claim()").unwrap();
    let normal = source.find("match ssh::dispatch").unwrap();
    assert!(dispatch < desktop_host && desktop_host < launcher && launcher < normal);
    assert!(source.contains("i32::from_ne_bytes(code.to_ne_bytes())"));
    assert!(
        !source.contains("shellfox_terminal::supervisor_entry()"),
        "mode policy belongs only to shared dispatcher"
    );
}
