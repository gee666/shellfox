use rusqlite::Connection;
use shellfox_host::*;
use std::path::PathBuf;

pub struct Scratch(pub PathBuf);
impl Scratch {
    pub fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("reserved-publication-{}", SessionId::new()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    pub fn db(&self) -> Connection {
        Connection::open(self.0.join(STORE_FILENAME)).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub fn input() -> ReservedSessionPublication {
    ReservedSessionPublication {
        request_id: SessionId::new(),
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        title: Some("external run".into()),
        cwd: "/work".into(),
        profile_id: ProfileId::new("login-shell").unwrap(),
    }
}
pub fn facts() -> SessionCreationFacts {
    SessionCreationFacts::new(SettingsAdapter::EmbeddedPty, SettingsShell::LoginShell)
}
pub fn ordinary(request_id: SessionId) -> CreateSession {
    CreateSession {
        request_id,
        title: None,
        cwd: "/work".into(),
        profile_id: ProfileId::default(),
    }
}
