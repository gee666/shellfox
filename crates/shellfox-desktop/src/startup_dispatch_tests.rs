#[test]
fn shells_are_supervised_by_the_console_cli_not_this_gui_binary() {
    // A windows-subsystem process inside ConPTY is not attached to the pseudoconsole.
    let main = include_str!("main.rs");
    assert!(main.contains("windows_subsystem = \"windows\""));
    assert!(!main.contains("dispatch_supervisor_entry"));
    assert!(!main.contains("supervisor_entry()"));
    assert!(include_str!("smoke.rs").contains("host_process::cli_path()"));
    let squirrel = main.find("squirrel::handle(event)").unwrap();
    let options = main.find("Options::parse(arguments)").unwrap();
    let startup = main.find("app_startup::prepare(&options)").unwrap();
    assert!(squirrel < options && options < startup);
}
