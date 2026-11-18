#[path = "../../native_app_composition.rs"]
mod app_composition;
mod args;
mod attach;
mod automation;
mod automation_args;
mod commands;
mod keys;
mod launch_args;
#[cfg(feature = "experimental-shellfox-native")]
mod peer;
#[cfg(feature = "experimental-shellfox-native")]
mod peer_args;
mod presentation;
mod queries;
mod query_rules;
mod remote_args;
mod remote_command;
#[cfg(test)]
mod startup_dispatch_tests;
mod terminal_text;
mod vt_bytes;

use anyhow::Result;

fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    match shellfox_service::ssh::dispatch_early_entry(&arguments) {
        Ok(Some(code)) => std::process::exit(i32::from_ne_bytes(code.to_ne_bytes())),
        Ok(None) => {}
        Err(error) => {
            eprintln!("shellfox-cli supervisor: {error:#}");
            std::process::exit(1);
        }
    }
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("shellfox-cli: {error:#}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<i32> {
    let Some(args) = args::parse(std::env::args_os().skip(1))? else {
        println!("{}", args::HELP);
        return Ok(0);
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    if matches!(&args.command, args::Command::Serve) {
        commands::serve(args, &runtime)
    } else {
        runtime.block_on(commands::run(args))
    }
}
