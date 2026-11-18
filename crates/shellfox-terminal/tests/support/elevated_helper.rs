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
        launch["type"] == "launch-isolated" && launch["isolationRoot"].is_string(),
        "invalid fixture launch"
    );
    if scenario == "cancelled" {
        send(json!({"type":"error", "code":"cancelled", "message":"owned fixture cancellation"}))?;
        send(json!({"type":"exit", "exitCode":1}))?;
        return Ok(());
    }
    send(json!({"type":"ready", "pid":424242, "birth":"133000000000000000"}))?;
    if scenario == "launcher-first" {
        Command::new(std::env::current_exe()?)
            .arg("--shellfox-elevated-fixture-broker")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::null())
            .spawn()?;
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
