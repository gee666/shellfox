//! Owned protocol fixture. It never starts a shell, UAC or a user command.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Read, Write},
    process::{Command, Stdio},
    time::Duration,
};

fn receive(reader: &mut impl BufRead) -> anyhow::Result<Option<Value>> {
    let mut frame = Vec::new();
    let count = reader.by_ref().take(262145).read_until(b'\n', &mut frame)?;
    if count == 0 {
        return Ok(None);
    }
    anyhow::ensure!(
        count <= 262144 && frame.last() == Some(&b'\n'),
        "invalid fixture frame"
    );
    Ok(Some(serde_json::from_slice(&frame)?))
}

fn send(value: Value) -> io::Result<()> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, &value)?;
    output.write_all(b"\n")?;
    output.flush()
}
/// The broker outlives this launcher and runs from the fixture directory, so
/// the test must wait for its real exit before removing that directory. Gives
/// the host a wait-only handle to it, listed in `owned-processes` in the cwd.
#[cfg(windows)]
fn hand_to_host(child: &std::process::Child) -> anyhow::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, DuplicateHandle},
        System::Threading::{
            GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE, SYNCHRONIZATION_SYNCHRONIZE,
        },
    };
    let host: u32 = std::env::var("SHELLFOX_FIXTURE_HOST_PID")?.parse()?;
    // SAFETY: plain handle calls; the host process handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, host) };
    anyhow::ensure!(!process.is_null(), "open fixture host");
    let mut handle = std::ptr::null_mut();
    // SAFETY: the child handle stays open for the call; the duplicate belongs to the host.
    let duplicated = unsafe {
        let current = GetCurrentProcess();
        let ok = DuplicateHandle(
            current,
            child.as_raw_handle().cast(),
            process,
            &mut handle,
            SYNCHRONIZATION_SYNCHRONIZE,
            0,
            0,
        );
        CloseHandle(process);
        ok
    };
    anyhow::ensure!(duplicated != 0, "hand fixture process to host");
    let mut list = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("owned-processes")?;
    writeln!(list, "{}", handle as usize)?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode] if mode == "--shellfox-elevated-fixture-peer" => {
            std::thread::sleep(Duration::from_millis(350));
            return Ok(());
        }
        [mode] if mode == "--shellfox-elevated-fixture-broker" => {
            let mut peer = Command::new(std::env::current_exe()?)
                .arg("--shellfox-elevated-fixture-peer")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            anyhow::ensure!(peer.wait()?.success(), "owned fixture peer failed");
            // Actual original peer handle has exited before this proof frame.
            send(json!({"type":"exit", "exitCode":0}))?;
            return Ok(());
        }
        [mode, _] if mode == "--shellfox-elevated-fixture" => {}
        _ => anyhow::bail!("expected fixed fake-helper fixture mode"),
    }
    let scenario = &args[1];
    let mut input = io::stdin().lock();
    let launch = receive(&mut input)?.ok_or_else(|| io::Error::other("missing fixture launch"))?;
    anyhow::ensure!(
        launch["type"] == "launch" && launch["cwd"].is_string(),
        "invalid fixture launch"
    );
    if scenario == "cancelled" {
        send(json!({"type":"error", "code":"cancelled", "message":"owned fixture cancellation"}))?;
        send(json!({"type":"exit", "exitCode":1}))?;
        return Ok(());
    }
    send(json!({"type":"ready", "pid":424242, "birth":"133000000000000000"}))?;
    if scenario == "launcher-first" {
        let broker = Command::new(std::env::current_exe()?)
            .arg("--shellfox-elevated-fixture-broker")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::null())
            .spawn()?;
        #[cfg(windows)]
        hand_to_host(&broker)?;
        drop(broker);
        return Ok(());
    }
    if scenario == "unconfirmed" {
        return Ok(());
    }
    anyhow::ensure!(scenario == "normal", "unknown fixture scenario");
    send(json!({"type":"data", "data":STANDARD.encode(b"OWNED_ELEVATED_FIXTURE")}))?;
    let mut last_id = 0u64;
    while let Some(message) = receive(&mut input)? {
        match message["type"].as_str() {
            Some("write") => {
                let id = message["id"]
                    .as_u64()
                    .ok_or_else(|| io::Error::other("invalid fixture write"))?;
                anyhow::ensure!(id > last_id, "unordered fixture write");
                last_id = id;
                send(json!({"type":"ack", "id":id}))?;
                let text = message["data"]
                    .as_str()
                    .ok_or_else(|| io::Error::other("invalid fixture data"))?;
                for chunk in text.as_bytes().chunks(4096) {
                    send(json!({"type":"data", "data":STANDARD.encode(chunk)}))?;
                }
            }
            Some("resize") => {}
            Some("close") => {
                send(json!({"type":"data", "data":STANDARD.encode(b"OWNED_FINAL_BYTES")}))?;
                send(json!({"type":"exit", "exitCode":0}))?;
                return Ok(());
            }
            _ => anyhow::bail!("invalid fixture command"),
        }
    }
    Ok(())
}
