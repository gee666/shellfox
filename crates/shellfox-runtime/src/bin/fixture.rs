//! Isolated headless runtime driver and controlled integration-test child.
//! The supervisor mode is checked before threads, parsing or runtime startup.
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_runtime::{Command, Event, Identity, Request, RequestId, Response, Runtime};
use shellfox_terminal::{PtyCommand, PtyConfig};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn main() -> Result<()> {
    if let Some(code) = shellfox_terminal::supervisor_entry()? {
        std::process::exit(code as i32);
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.first().and_then(|arg| arg.to_str()) {
        Some("--child-final") if args.len() == 1 => {
            print!("\x1b[32mRUNTIME_FINAL_BYTES\x1b[0m\r\n");
            io::stdout().flush()?;
        }
        Some("--child-echo") if args.len() == 1 => {
            println!("RUNTIME_READY");
            io::stdout().flush()?;
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            println!("RUNTIME_ECHO={}", line.trim_end());
        }
        Some("--child-query") if args.len() == 1 => query_child()?,
        Some("--child-flood") if args.len() == 1 => flood()?,
        Some("--child-flood-on-input") if args.len() == 1 => {
            println!("RUNTIME_FLOOD_READY");
            io::stdout().flush()?;
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            flood()?;
        }
        Some("--child-idle-escapes") if args.len() == 1 => {
            idle_escapes()?;
            loop {
                std::thread::park();
            }
        }
        Some("--child-idle-escapes-repeat") if args.len() == 1 => {
            let mut row = 1;
            loop {
                idle_escapes_at(row)?;
                row = if row == 1 { 2 } else { 1 };
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        Some("--child-visibility-escapes-repeat") if args.len() == 1 => {
            let mut visible = true;
            loop {
                io::stdout().write_all(if visible { b"\x1b[?25h" } else { b"\x1b[?25l" })?;
                io::stdout().flush()?;
                visible = !visible;
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        Some("--child-presentations") if args.len() == 1 => {
            for row in 0..40 {
                print!("row{row:02}\r\n");
            }
            print!("\x1b]2;presentation-fixture\x07\x07\x1b]7;file://foreign.example/project\x07PRESENTATION_READY");
            io::stdout().flush()?;
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            println!("\r\nPRESENTATION_INPUT={}", line.trim_end());
        }
        Some("--owned-agent") if args.len() == 1 => {
            println!("OWNED_AGENT_READY");
            io::stdout().flush()?;
            std::thread::sleep(Duration::from_secs(30));
        }
        Some("--child-inspection") if args.len() == 1 => {
            let mut child = std::process::Command::new(std::env::current_exe()?)
                .arg("--owned-agent")
                .spawn()?;
            println!("INSPECTION_ROOT_READY");
            io::stdout().flush()?;
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            println!("INSPECTION_TYPING={}", line.trim_end());
            io::stdout().flush()?;
            child.wait()?;
        }
        Some("--child-bracketed-idle") if args.len() == 1 => {
            io::stdout().write_all(b"\x1b[?2004hRUNTIME_BRACKETED_READY")?;
            io::stdout().flush()?;
            loop {
                std::thread::park();
            }
        }
        Some("--child-escape-idle") if args.len() == 1 => {
            io::stdout().write_all(b"\x1b[0m\x1b[?25h\x1b[5n")?;
            io::stdout().flush()?;
            std::thread::sleep(Duration::from_secs(30));
        }
        Some(mode @ ("--child-mouse-x10" | "--child-mouse-sgr")) if args.len() == 1 => {
            fixture_vt_input()?;
            io::stdout().write_all(b"\x1b[?1000h")?;
            if mode == "--child-mouse-sgr" {
                io::stdout().write_all(b"\x1b[?1006h")?;
            }
            println!("RUNTIME_MOUSE_READY");
            io::stdout().flush()?;
            std::thread::sleep(Duration::from_secs(30));
        }
        Some("--child-input-boundary") if args.len() == 1 => {
            fixture_vt_input()?;
            io::stdout().write_all(b"\x1b[?2004h\x1b[>31uRUNTIME_INPUT_BOUNDARY_READY")?;
            io::stdout().flush()?;
            let mut bytes = [0; 8192];
            while io::stdin().read(&mut bytes)? != 0 {}
        }
        Some("--child-idle") if args.len() == 1 => {
            println!("RUNTIME_IDLE_READY");
            io::stdout().flush()?;
            std::thread::sleep(Duration::from_secs(30));
        }
        Some("--headless-check") if args.len() == 2 => run_check(PathBuf::from(&args[1]))?,
        _ => return Err(
            "usage: shellfox-runtime-fixture --headless-check <existing absolute isolated root>"
                .into(),
        ),
    }
    Ok(())
}
fn fixture_vt_input() -> Result<()> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::*;
        let (input, output) = (
            GetStdHandle(STD_INPUT_HANDLE),
            GetStdHandle(STD_OUTPUT_HANDLE),
        );
        let (mut input_mode, mut output_mode) = (0, 0);
        if GetConsoleMode(input, &mut input_mode) == 0
            || GetConsoleMode(output, &mut output_mode) == 0
            || SetConsoleMode(
                input,
                (input_mode | ENABLE_VIRTUAL_TERMINAL_INPUT)
                    & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT),
            ) == 0
            || SetConsoleMode(output, output_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) == 0
        {
            return Err(io::Error::last_os_error().into());
        }
    }
    Ok(())
}
fn run_check(root: PathBuf) -> Result<()> {
    if !root.is_absolute() || !root.is_dir() {
        return Err("isolated root must exist and be absolute".into());
    }
    let executable = std::env::current_exe()?;
    let mut config = PtyConfig::new(root);
    config.supervisor = Some(executable.clone());
    config.command = Some(PtyCommand {
        program: executable.into_os_string(),
        args: vec!["--child-final".into()],
    });
    let runtime = Runtime::start(
        Identity {
            session_id: SessionId::new(),
            tab_id: TabId::new(),
            generation: Generation::new(),
        },
        config,
    )?;
    runtime
        .try_send(Request {
            id: RequestId(1),
            command: Command::Claim {
                controller: 1,
                takeover: false,
            },
        })
        .map_err(|e| format!("claim submission: {e:?}"))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut next = 10;
    let mut done = false;
    while Instant::now() < deadline {
        while let Ok(event) = runtime.try_recv() {
            match event {
                Event::Started(Err(error)) => return Err(error.into()),
                Event::Reply {
                    result: Ok(Response::Claimed(lease)),
                    ..
                } => {
                    runtime
                        .try_send(Request {
                            id: RequestId(2),
                            command: Command::Replay { lease, after: 0 },
                        })
                        .map_err(|e| format!("replay submission: {e:?}"))?;
                }
                Event::Reply {
                    result: Ok(Response::Replay(chunks)),
                    ..
                } => {
                    for chunk in chunks {
                        io::stdout().write_all(&chunk.bytes)?;
                    }
                }
                Event::Output { lease, chunk } => {
                    io::stdout().write_all(&chunk.bytes)?;
                    runtime
                        .try_send(Request {
                            id: RequestId(next),
                            command: Command::Ack {
                                lease,
                                sequence: chunk.sequence,
                            },
                        })
                        .map_err(|e| format!("ACK submission: {e:?}"))?;
                    next += 1;
                }
                Event::Reply {
                    result: Err(error), ..
                } => return Err(error.into()),
                Event::Exited { .. } => done = true,
                _ => {}
            }
        }
        if done {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Err("headless runtime timed out; dropping handle requests retained cleanup".into())
}
fn idle_escapes() -> Result<()> {
    idle_escapes_at(1)
}
fn idle_escapes_at(row: u8) -> Result<()> {
    write!(io::stdout(), "\x1b[?25h\x1b[?2026h\x1b[{row};1H\x1b[?2026l")?;
    io::stdout().flush()?;
    Ok(())
}
fn flood() -> Result<()> {
    let bytes = [b'x'; 4096];
    for _ in 0..600 {
        io::stdout().write_all(&bytes)?;
    }
    print!("\r\nRUNTIME_FLOOD_FINAL\r\n");
    io::stdout().flush()?;
    Ok(())
}
fn query_child() -> Result<()> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::*;
        let input = GetStdHandle(STD_INPUT_HANDLE);
        let mut mode = 0;
        if GetConsoleMode(input, &mut mode) == 0 {
            return Err(io::Error::last_os_error().into());
        }
        if SetConsoleMode(
            input,
            (mode | ENABLE_VIRTUAL_TERMINAL_INPUT) & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT),
        ) == 0
        {
            return Err(io::Error::last_os_error().into());
        }
    }
    io::stdout().write_all(b"\x1b[6n")?;
    io::stdout().flush()?;
    let mut reply = Vec::new();
    while reply.len() < 64 {
        let mut byte = [0];
        io::stdin().read_exact(&mut byte)?;
        reply.push(byte[0]);
        if byte[0] == b'R' {
            break;
        }
    }
    println!("QUERY_REPLY={reply:?}\r\nRUNTIME_QUERY_FINAL");
    Ok(())
}
