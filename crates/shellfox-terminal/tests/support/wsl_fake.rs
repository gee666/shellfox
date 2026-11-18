//! A test-only executable. Never invokes wsl.exe, a guest interpreter or a shell.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::io::{Read, Write};
fn read() -> Value {
    let mut header = [0; 4];
    std::io::stdin().read_exact(&mut header).unwrap();
    let count = u32::from_be_bytes(header) as usize;
    assert!(count <= 96 * 1024);
    let mut data = vec![0; count];
    std::io::stdin().read_exact(&mut data).unwrap();
    serde_json::from_slice(&data).unwrap()
}
fn emit(value: Value) {
    let data = serde_json::to_vec(&value).unwrap();
    let mut output = std::io::stdout().lock();
    output
        .write_all(&(data.len() as u32).to_be_bytes())
        .unwrap();
    output.write_all(&data).unwrap();
    output.flush().unwrap();
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--list", "--quiet"] {
        let bytes: Vec<u8> = "\u{feff}Fixture\r\n测试\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        std::io::stdout().write_all(&bytes).unwrap();
        return;
    }
    assert_eq!(args[0], "--distribution");
    assert_eq!(args[2], "--exec");
    assert!(args.windows(2).any(|a| a == ["-I", "-S"]));
    let config = read();
    if config["mode"] == "probe" {
        emit(
            json!({"type":"capability","protocol":1,"boot":"12345678-1234-1234-1234-123456789abc","pidfd":true,"subreaper":true,"peercred":true}),
        );
        return;
    }
    let anchor = if config["mode"] == "recover" {
        config["anchor"].clone()
    } else {
        json!({"bootId":"12345678-1234-1234-1234-123456789abc","guardianPid":10,"guardianTicks":"100","marker":config["marker"]})
    };
    let identity = json!({"bootId":anchor["bootId"],"guardianPid":anchor["guardianPid"],"guardianTicks":anchor["guardianTicks"],"rootPid":11,"rootTicks":"101","session":11,"marker":anchor["marker"]});
    let status = |zero: bool| json!({"identity":identity,"owned":if zero{0}else{1},"cleanupConfirmed":zero,"rootExit":if zero{json!(0)}else{Value::Null}});
    if config["mode"] == "recover" {
        assert_eq!(config["secret"].as_str().unwrap().len(), 64);
        let zero = config["action"] == "close" && args[1] != "CleanupFailure";
        emit(json!({"type":"reply","id":1,"status":status(zero)}));
        return;
    }
    let endpoint = format!("{}/control.sock", config["state"].as_str().unwrap());
    emit(json!({"type":"prepared","anchor":anchor,"endpoint":endpoint}));
    assert_eq!(read(), json!({"action":"activate"}));
    emit(json!({"type":"ready","status":status(false),"endpoint":endpoint}));
    emit(json!({"type":"data","sequence":1,"data":STANDARD.encode(b"\x1b[31mguest\x1b[0m\r\n")}));
    let mut closes = 0;
    loop {
        let command = read();
        match command["action"].as_str().unwrap() {
            "close" => {
                closes += 1;
                let zero = args[1] != "CleanupFailure" || closes > 1;
                emit(json!({"type":"reply","id":command["id"],"status":status(zero)}));
                if zero {
                    emit(json!({"type":"stopped","status":status(true)}));
                    emit(json!({"type":"eof"}));
                    return;
                }
            }
            "status" => emit(json!({"type":"reply","id":command["id"],"status":status(false)})),
            "input" => {
                let bytes = STANDARD.decode(command["data"].as_str().unwrap()).unwrap();
                emit(json!({"type":"input_ack","bytes":bytes.len()}));
            }
            "ack" | "resize" => {}
            _ => panic!("unexpected fixture action"),
        }
    }
}
