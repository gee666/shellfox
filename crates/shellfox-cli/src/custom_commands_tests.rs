use super::*;

fn run_words(store: &Store, words: &[&str]) -> Result<String> {
    let words: Vec<String> = words.iter().map(|w| (*w).to_owned()).collect();
    let mut output = String::new();
    dispatch(store, &words, &mut output)?;
    Ok(output)
}

#[test]
fn registry_round_trip_matches_0_9_files_and_messages() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store(dir.path().to_owned());
    let script = dir.path().join(if cfg!(windows) {
        "build.ps1"
    } else {
        "build.sh"
    });
    std::fs::write(&script, "").unwrap();
    let path = script.to_str().unwrap();
    assert_eq!(
        run_words(&store, &["--list"]).unwrap(),
        "No custom commands.\n"
    );
    assert_eq!(
        run_words(&store, &["--register", "Build", path]).unwrap(),
        "Registered \"Build\".\n"
    );
    assert!(run_words(&store, &["--register", "build", path]).is_err());
    assert_eq!(run_words(&store, &["--complete", "b"]).unwrap(), "Build\n");
    assert_eq!(
        run_words(&store, &["--rename", "build", "ship"]).unwrap(),
        "Renamed \"build\" to \"ship\".\n"
    );
    assert!(run_words(&store, &["--ls"])
        .unwrap()
        .starts_with("NAME  PATH\nship  \""));
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("cli-commands.json")).unwrap())
            .unwrap();
    assert_eq!(saved["version"], 1);
    assert_eq!(saved["commands"][0]["name"], "ship");
    assert_eq!(
        run_words(&store, &["--rm", "SHIP"]).unwrap(),
        "Removed \"SHIP\".\n"
    );
    assert!(run_words(&store, &["--register", "x"])
        .unwrap_err()
        .is::<Usage>());
    assert!(!run_words(&store, &["missing"]).unwrap_err().is::<Usage>());
    assert!(!dir.path().join("cli-commands.json.lock").exists());
}

#[test]
fn a_0_9_registry_with_a_wsl_entry_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("cli-commands.json");
    std::fs::write(
        &file,
        r#"{"version":1,"commands":[{"name":"lint","path":"/home/u/lint.sh","distro":"Ubuntu"}]}"#,
    )
    .unwrap();
    let store = Store(dir.path().to_owned());
    assert_eq!(
        store.find("LINT").unwrap().location.distro.as_deref(),
        Some("Ubuntu")
    );
    assert!(run_words(&store, &["--list"])
        .unwrap()
        .starts_with("NAME  PATH               WSL\n"));
    std::fs::write(
        &file,
        r#"{"version":1,"commands":[{"name":"x","path":"rel"}]}"#,
    )
    .unwrap();
    assert!(store.read().is_err());
}

#[test]
fn a_registered_script_gets_literal_arguments_and_its_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store(dir.path().to_owned());
    let out = dir.path().join("out.txt");
    let (name, body) = if cfg!(windows) {
        (
            "echo.ps1",
            format!(
                "Set-Content -LiteralPath '{}' -Value ($args -join '|'); exit 3",
                out.display()
            ),
        )
    } else {
        (
            "echo.sh",
            format!("printf '%s|' \"$@\" > '{}'; exit 3", out.display()),
        )
    };
    let script = dir.path().join(name);
    std::fs::write(&script, body).unwrap();
    run_words(&store, &["--register", "echo", script.to_str().unwrap()]).unwrap();
    let words: Vec<String> = ["echo", "a b", "--json", "'$x"].map(str::to_owned).to_vec();
    let mut output = String::new();
    assert_eq!(dispatch(&store, &words, &mut output).unwrap(), Some(3));
    let written = std::fs::read_to_string(&out).unwrap();
    assert!(written.starts_with("a b|--json|'$x"), "{written}");
}
