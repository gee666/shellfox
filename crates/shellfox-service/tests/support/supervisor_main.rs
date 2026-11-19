// Run the same early entry used by the CLI and desktop, without starting a host.
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let code = match shellfox_service::dispatch_supervisor_entry() {
        Ok(Some(code)) => i32::from_ne_bytes(code.to_ne_bytes()),
        Ok(None) if args.len() == 1 && args[0] == "--child-echo" => echo(),
        Ok(None) => 2,
        Err(error) => {
            eprintln!("{error:#}");
            1
        }
    };
    std::process::exit(code);
}

fn echo() -> i32 {
    use std::io::Write;
    println!("RUNTIME_READY");
    let mut line = String::new();
    if std::io::stdout().flush().is_err() || std::io::stdin().read_line(&mut line).is_err() {
        return 1;
    }
    println!("RUNTIME_ECHO={}", line.trim_end());
    0
}
