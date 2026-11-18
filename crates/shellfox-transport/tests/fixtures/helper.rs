//! Offline executable fixture. This binary never embeds or enrolls Tailscale.
#[path = "lifecycle.rs"]
mod lifecycle;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
fn emit(value: Value) {
    println!("{value}");
    std::io::stdout().flush().unwrap();
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|v| v == "--probe") {
        lifecycle::probe_hold(std::path::Path::new(&args[0]));
        emit(
            json!({"protocolVersion":1,"tailscaleVersion":"1.104.2","platform":if cfg!(windows){"windows"} else if cfg!(target_os="macos"){"darwin"}else{"linux"},"arch":"amd64"}),
        );
        return;
    }
    if args.get(1).is_some_and(|v| v == "--private-permissions") {
        emit(json!({"ok":true}));
        return;
    }
    if args.get(1).is_some_and(|v| v == "--ownership") {
        lifecycle::ownership(std::path::Path::new(&args[2]));
        return;
    }
    let stdin = std::io::stdin();
    let mut mode = String::new();
    let mut directory = std::path::PathBuf::new();
    for line in stdin.lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let id = request["id"].as_str().unwrap();
        let payload = &request["payload"];
        let method = request["method"].as_str().unwrap();
        let value = match method {
            "start" => {
                assert!(payload.get("authKey").is_none());
                mode = payload["hostname"].as_str().unwrap().to_owned();
                directory = payload["stateDirectory"].as_str().unwrap().into();
                lifecycle::started(&mode, &directory);
                let secret = payload["proxySecret"].as_str().unwrap().to_owned();
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let address = listener.local_addr().unwrap();
                let calls = Arc::new(AtomicUsize::new(0));
                std::thread::spawn(move || {
                    for stream in listener.incoming() {
                        let stream = stream.unwrap();
                        let secret = secret.clone();
                        let calls = calls.clone();
                        std::thread::spawn(move || proxy(stream, &secret, &calls));
                    }
                });
                emit(
                    json!({"event":"status","payload":{"status":"online","loginUrl":null,"error":null}}),
                );
                if mode == "start-invalid" {
                    json!({"proxyUrl":"http://127.0.0.1:0"})
                } else {
                    json!({"proxyUrl":format!("http://{address}")})
                }
            }
            "status" | "login" | "logout" => {
                if mode == "wrong-id" {
                    emit(json!({"id":"999","ok":true,"value":{}}));
                    continue;
                }
                if mode == "malformed" {
                    println!("{{bad secret auth tskey-auth-DO-NOT-LOG");
                    std::io::stdout().flush().unwrap();
                    continue;
                }
                if mode == "huge-line" {
                    println!("{}", "x".repeat(1024 * 1024 + 1));
                    std::io::stdout().flush().unwrap();
                    continue;
                }
                if mode == "secret-error" {
                    eprintln!("{}", "tskey-auth-DO-NOT-LOG".repeat(10000));
                    emit(
                        json!({"id":id,"ok":false,"error":{"code":"helper-error","message":"tskey-auth-DO-NOT-LOG"}}),
                    );
                    continue;
                }
                if method == "login" {
                    json!({"status":"login-required","loginUrl":"https://login.tailscale.com/a/test-only","error":null})
                } else {
                    json!({"status":"online","loginUrl":null,"error":null})
                }
            }
            "peers" => {
                json!({"peers":[{"id":"n-peer","hostname":"my laptop","addresses":["100.64.0.2"],"online":true}]})
            }
            "stop" => {
                if lifecycle::stop(&mode, &directory, id) {
                    return;
                }
                emit(json!({"id":id,"ok":true,"value":{"stopped":true}}));
                return;
            }
            _ => panic!("unexpected fixture method"),
        };
        emit(json!({"id":id,"ok":true,"value":value}));
    }
}
fn proxy(mut stream: std::net::TcpStream, secret: &str, calls: &AtomicUsize) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    assert_eq!(first, "POST /proxy HTTP/1.1\r\n");
    let mut length = 0;
    let mut authorized = false;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            length = value.trim().parse::<usize>().unwrap();
        }
        if line == format!("Authorization: Bearer {secret}\r\n") {
            authorized = true;
        }
    }
    assert!(authorized && length <= 1024 * 1024);
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).unwrap();
    let request: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(request["peerId"], "n-peer");
    assert!([
        "/v1/discover",
        "/v1/pair",
        "/v1/request",
        "/v1/events",
        "/v1/disconnect"
    ]
    .contains(&request["path"].as_str().unwrap()));
    let calls = calls.fetch_add(1, Ordering::SeqCst) + 1;
    let case = request["body"]["case"].as_str().unwrap_or("normal");
    if case == "drop" {
        return;
    }
    if case == "slow" {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    let body = match case {
        "approval" => {
            br#"{"ok":false,"error":{"code":"APPROVAL_REQUIRED","message":"pending","retryable":true}}"#
                .to_vec()
        }
        "revoked" => {
            br#"{"ok":false,"error":{"code":"ACCESS_DENIED","message":"revoked","retryable":false}}"#.to_vec()
        }
        "huge" => vec![b' '; 8 * 1024 * 1024 + 1],
        "count" => serde_json::to_vec(&json!({"calls":calls})).unwrap(),
        _ => br#"{"ok":true,"value":{}}"#.to_vec(),
    };
    let status = if case == "redirect" {
        "302 Found\r\nLocation: http://127.0.0.1:1/no-replay"
    } else {
        "200 OK"
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(&body);
}
