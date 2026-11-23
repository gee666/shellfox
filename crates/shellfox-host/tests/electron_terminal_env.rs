//! 0.9 terminal variables, PATH and NODE_OPTIONS included, import and reach tabs.
use rusqlite::Connection;
use shellfox_host::*;
use std::path::PathBuf;

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn imported_terminal_env_with_path_and_node_options_reaches_the_launch() {
    let scratch = Scratch(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("electron-terminal-env-{}", SessionId::new())),
    );
    std::fs::create_dir_all(scratch.0.join("host")).unwrap();
    let snapshot = scratch.0.join("copy.sqlite3");
    let db = Connection::open(&snapshot).unwrap();
    db.execute_batch(include_str!("fixtures/legacy-v1.sql")).unwrap();
    db.pragma_update(None, "user_version", 5).unwrap();
    let env = vec![
        EnvVar { name: "PATH".into(), value: "/opt/tools/bin:/usr/bin".into() },
        EnvVar { name: "NODE_OPTIONS".into(), value: "--max-old-space-size=8192".into() },
        EnvVar { name: "PSModulePath".into(), value: "/opt/modules".into() },
    ];
    let settings = Settings { terminal_env: env.clone(), ..Default::default() };
    db.execute(
        "UPDATE settings SET value=?",
        [serde_json::to_string(&settings).unwrap()],
    )
    .unwrap();
    drop(db);

    let host = Host::open(scratch.0.join("host")).unwrap();
    assert!(host.import_electron_settings(&snapshot).unwrap());
    let created = host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: "/work".into(),
            profile_id: ProfileId::default(),
        })
        .unwrap();
    let tab = created.selection.tab_id.unwrap();
    assert_eq!(host.terminal_launch(tab).unwrap().env, env);
}
