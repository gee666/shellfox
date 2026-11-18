#[test]
fn privileged_discriminators_precede_normal_args_prestart_and_graphics() {
    let source = include_str!("main.rs");
    let dispatch = source
        .find("shellfox_service::ssh::dispatch_early_entry(&arguments)")
        .unwrap();
    let options = source.find("Options::parse(arguments)").unwrap();
    let prestart = source.find("app_startup::prepare(&options)").unwrap();
    assert!(dispatch < options && options < prestart);
    assert!(source.contains("i32::from_ne_bytes(code.to_ne_bytes())"));
    assert!(
        !source.contains("shellfox_terminal::supervisor_entry()"),
        "mode policy belongs only to shared dispatcher"
    );
}
