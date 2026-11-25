use shellfox_service::app_composition;
mod args;
mod attach;
mod auth_key;
mod automation;
mod automation_args;
mod commands;
mod completion;
mod custom_commands;
mod desktop_handoff;
mod desktop_host;
mod headless_layout;
mod headless_package;
mod headless_update;
mod help;
mod host_service;
mod human;
mod human_text;
mod keys;
mod launch_args;
mod legacy_host;
mod manager;
mod manager_args;
mod manager_local;
mod manager_local_files;
mod manager_local_terminal;
mod manager_peer;
mod manager_run;
mod node_args;
mod node_run;
mod output;
mod parent_watch;
mod presentation;
mod queries;
mod query_rules;
mod remote_args;
mod remote_attach;
mod remote_command;
mod session_view;
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
    if arguments
        .first()
        .is_some_and(|flag| flag == shellfox_service::local::LocalBootstrap::DESKTOP_HOST_FLAG)
    {
        // A Node launcher's IPC channel belongs to it, not to this host or its tabs.
        for key in ["NODE_CHANNEL_FD", "NODE_CHANNEL_SERIALIZATION_MODE"] {
            std::env::remove_var(key);
        }
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
        None => {
            let json = arguments
                .iter()
                .take_while(|a| *a != "--")
                .any(|a| a == "--json");
            output::set_mode(if json {
                output::Mode::Json
            } else {
                output::Mode::Human
            });
            run(arguments, watch).unwrap_or_else(|error| output::print_error(&error))
        }
    };
    std::process::exit(code);
}

/// One command grammar. Explicit --data-dir/--helper selection may precede it.
fn run(arguments: Vec<OsString>, watch: Option<parent_watch::Watch>) -> Result<i32> {
    let mut words = arguments.into_iter().peekable();
    let selection = args::Selection::parse(&mut words)?;
    let invocation = node_args::parse(&words.collect::<Vec<_>>())?;
    // A script's own --json is not ours: only parsed globals pick the output.
    output::set_mode(if invocation.json {
        output::Mode::Json
    } else {
        output::Mode::Human
    });
    node_run::run(invocation, selection, watch)
}
