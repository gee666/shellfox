use super::support::Fixture;
use std::fs;

#[test]
fn listed_hardlink_is_rejected_even_without_an_extra_directory_entry() {
    let fixture = Fixture::new();
    let lazy = fixture.root.join("nested/lazy.dll");
    fs::remove_file(&lazy).unwrap();
    fs::hard_link(fixture.root.join("Shellfox.ElevatedTerminal.dll"), &lazy).unwrap();
    assert!(fixture.open().is_err());
}

#[cfg(windows)]
#[test]
fn junction_parent_cannot_redirect_a_manifest_payload() {
    use std::{os::windows::process::CommandExt, process::Command};
    let fixture = Fixture::new();
    let outside = Fixture::new();
    fs::remove_dir_all(fixture.root.join("nested")).unwrap();
    let quote = |path: std::path::PathBuf| path.to_string_lossy().replace('\'', "''");
    let result = Command::new(
        std::path::Path::new(&std::env::var_os("SystemRoot").unwrap()).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    )
    .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &format!(
        "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
        quote(fixture.root.join("nested")), quote(outside.root.join("nested"))
    )])
    .creation_flags(0x08000000)
    .output()
    .unwrap();
    assert!(
        result.status.success(),
        "synthetic junction prerequisite: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(fixture.open().is_err());
    fs::remove_dir(fixture.root.join("nested")).unwrap();
}
