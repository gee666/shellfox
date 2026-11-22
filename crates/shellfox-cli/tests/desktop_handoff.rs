//! A desktop-installed CLI forwards update and cold session launch to the
//! sibling desktop binary. The stub desktop is a copy of this CLI: it rejects
//! `--cli-update --check` with a usage error on stderr and exit code 2, which
//! proves the arguments, inherited output and exit code are passed through.
use std::{fs, path::Path, process::Command};

fn install(root: &Path) -> std::path::PathBuf {
    let built = Path::new(env!("CARGO_BIN_EXE_shellfox-cli"));
    let bin = root.join("app-0.10.0").join("bin");
    fs::create_dir_all(&bin).unwrap();
    let (cli, desktop) = if cfg!(windows) {
        (
            bin.join("shellfox-cli.exe"),
            root.join("app-0.10.0").join("Shellfox.exe"),
        )
    } else if cfg!(target_os = "macos") {
        (bin.join("shellfox-cli"), bin.join("Shellfox"))
    } else {
        (bin.join("shellfox-cli"), bin.join("shellfox"))
    };
    fs::copy(built, &cli).unwrap();
    fs::copy(built, &desktop).unwrap();
    if cfg!(windows) {
        let dll = built.with_file_name("ghostty-vt.dll");
        for dir in [cli.parent().unwrap(), desktop.parent().unwrap()] {
            fs::copy(&dll, dir.join("ghostty-vt.dll")).unwrap();
        }
    }
    cli
}

#[test]
fn desktop_update_and_cold_launch_reach_the_sibling_desktop() {
    let scratch = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("tmp");
    fs::create_dir_all(&scratch).unwrap();
    let temp = tempfile::Builder::new()
        .prefix("desktop-handoff-")
        .tempdir_in(scratch)
        .unwrap();
    let cli = install(temp.path());
    let data = temp.path().join("user data");
    let run = |args: &[&str]| {
        Command::new(&cli)
            .args(args)
            .env("SHELLFOX_USER_DATA", &data)
            .output()
            .unwrap()
    };

    let forwarded = run(&["update", "--check"]);
    assert_eq!(forwarded.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&forwarded.stderr);
    assert!(
        stderr.contains("Unknown or unsupported command"),
        "{stderr}"
    );

    let json = run(&["update", "--json"]);
    assert_eq!(json.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&json.stdout).contains("\"code\":\"VALIDATION\""));

    let remote = run(&["--host", "other", "update"]);
    assert_ne!(remote.status.code(), Some(0));

    // No host is running: the desktop is started detached instead of failing.
    let cold = run(&["session", "create", temp.path().to_str().unwrap()]);
    assert_eq!(
        cold.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&cold.stderr)
    );
    assert!(!data.join("rust-v1").exists(), "no host was started");
    let json_create = run(&["session", "create", ".", "--json"]);
    // JSON never opens a window; it reports the missing host as a failure.
    assert_eq!(json_create.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&json_create.stdout).starts_with("{\"error\""));
}
