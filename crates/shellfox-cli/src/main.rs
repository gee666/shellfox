use shellfox_service::app_composition;
mod args;
mod attach;
mod automation;
mod automation_args;
mod commands;
mod desktop_handoff;
mod desktop_host;
mod headless_layout;
mod headless_package;
mod headless_update;
mod host_service;
mod keys;
mod launch_args;
mod legacy_host;
mod node_args;
mod node_run;
mod output;
mod parent_watch;
mod presentation;
mod queries;
mod query_rules;
mod remote_args;
mod remote_command;
mod ssh;
#[cfg(test)]
mod startup_dispatch_tests;
mod terminal_text;
mod time_text;
mod vt_bytes;

use anyhow::Result;
use std::ffi::OsString;

fn main() {
    match shellfox_service::dispatch_supervisor_entry() {
        Ok(Some(code)) => std::process::exit(i32::from_ne_bytes(code.to_ne_bytes())),
        Ok(None) => {}
        Err(error) => {
            eprintln!("shellfox-cli supervisor: {error:#}");
            std::process::exit(1);
        }
    }
    let mut arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    if arguments.first().is_some_and(|flag| flag == shellfox_service::local::LocalBootstrap::DESKTOP_HOST_FLAG) {
        std::process::exit(desktop_host::run(&arguments[1..]));
    }
    let watch = parent_watch::claim();
    headless_layout::strip_node_entry(&mut arguments);
    match headless_layout::forward(&arguments) {
        Ok(Some(code)) => std::process::exit(code),
        Ok(None) => {}
        Err(error) => {
            eprintln!("Shellfox: {error:#}");
            std::process::exit(1);
        }
    }
    let code = match ssh::dispatch(&arguments) {
        Some(result) => result.unwrap_or_else(|error| output::print_error(&error)),
        None if experimental(&arguments) => run(arguments).unwrap_or_else(|error| output::print_error(&error)),
        None => {
            let json = arguments.iter().take_while(|a| *a != "--").any(|a| a == "--json");
            output::set_mode(if json { output::Mode::Json } else { output::Mode::Human });
            node_args::parse(&arguments)
                .and_then(|invocation| node_run::run(invocation, watch))
                .unwrap_or_else(|error| output::print_error(&error))
        }
    };
    std::process::exit(code);
}

/// Development and package smoke tests select the host with explicit paths.
fn experimental(arguments: &[OsString]) -> bool {
    arguments.first().is_some_and(|first| {
        first == "--data-dir"
            || first == "--helper"
            || first == "--helper-sha256"
            || app_composition::ApplicationArguments::is_flag(first)
    })
}

fn run(arguments: Vec<OsString>) -> Result<i32> {
    let Some(args) = args::parse(arguments)? else {
        println!("{}", args::HELP);
        return Ok(0);
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    if matches!(&args.command, args::Command::Serve) {
        commands::serve(args, &runtime, || {
            eprintln!("Host ready. Remote backend available; network activation remains explicit.")
        })
    } else {
        runtime.block_on(commands::run(args))
    }
}
