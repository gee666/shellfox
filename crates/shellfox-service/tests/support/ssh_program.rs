// Standalone std-only fake SSH. It never resolves/connects/logs in or reads account keys.
use std::io::Write;
fn main(){
    assert!(std::env::var("SSH_ASKPASS_REQUIRE").as_deref()==Ok("force"),"missing target callback mode");
    let capability=std::env::var("SHELLFOX_SSH_ASKPASS_TOKEN").expect("missing target callback capability");
    assert!(capability.len()==64 && capability.bytes().all(|b|b.is_ascii_hexdigit()),"invalid target callback capability");
    assert!(std::env::var_os("SHELLFOX_SSH_ASKPASS_ENDPOINT").is_some(),"missing target callback endpoint");
    assert!(std::env::var_os("SHELLFOX_SSH_ASKPASS_DIRECTORY").is_some(),"missing target callback directory");
    let gate=std::env::current_exe().unwrap().parent().unwrap().join("auth-pending");
    while gate.exists(){std::thread::sleep(std::time::Duration::from_millis(10));}
    let helper=std::env::var_os("SSH_ASKPASS").expect("configured askpass helper");
    let mut answer=std::process::Command::new(helper).arg("--shellfox-pty-supervisor").output().expect("owned configured helper");
    assert!(answer.status.success() && answer.stdout==b"synthetic-native-answer\n" && answer.stderr.is_empty(),"owned callback answer failed");
    answer.stdout.fill(0);answer.stderr.fill(0);
    let command=std::env::args().next_back().expect("remote fixture command");
    let marker=command.split("printf '\\033").nth(1).and_then(|s|s.split("\\007'").next()).expect("fixture proof marker");
    std::io::stdout().write_all(format!("\x1b{marker}\x07SSH_NATIVE_FIXTURE_READY\r\n").as_bytes()).unwrap();
    std::io::stdout().flush().unwrap();
    // Stay alive until owned close, like an interactive remote shell.
    std::thread::sleep(std::time::Duration::from_secs(60));
}
