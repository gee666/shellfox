//! Separate from the catalog/probe tests to keep each test file below 350 lines.
use shellfox_core::{ErrorCode, Result, SessionId, Settings, SettingsPatch};
use shellfox_host::{Host, SettingsRuntime};
use shellfox_protocol::CancellationToken;
use shellfox_service::settings_runtime::SettingsRuntimeAdapter;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Copy)]
enum LoseAt {
    NormalizationReturn,
    Commit,
}
struct LosingRuntime {
    adapter: SettingsRuntimeAdapter,
    lost: CancellationToken,
    at: LoseAt,
    commits: Arc<AtomicUsize>,
}
impl SettingsRuntime for LosingRuntime {
    fn normalize(&self, previous: &Settings, candidate: Settings) -> Result<Settings> {
        let settings = self.adapter.normalize(previous, candidate)?;
        // Simulates the monitor firing after bounded work, before the Host writes.
        if matches!(self.at, LoseAt::NormalizationReturn) {
            self.lost.cancel();
        }
        Ok(settings)
    }
    fn check_commit(&self) -> Result<()> {
        self.commits.fetch_add(1, Ordering::SeqCst);
        // This callback runs after all SQL writes, under the transaction lock.
        if matches!(self.at, LoseAt::Commit) {
            self.lost.cancel();
        }
        self.adapter.check_commit()
    }
}
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("settings-runtime-guard-{}", SessionId::new()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn rejects_lost_commit(at: LoseAt) {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let before = host.global_settings().unwrap();
    let snapshot = host.snapshot().unwrap();
    let lost = CancellationToken::new();
    let commits = Arc::new(AtomicUsize::new(0));
    let adapter = SettingsRuntimeAdapter::new(lost.clone());
    host.install_settings_runtime(Arc::new(LosingRuntime {
        adapter,
        lost: lost.clone(),
        at,
        commits: commits.clone(),
    }))
    .unwrap();
    assert_eq!(
        host.save_global_settings(SettingsPatch {
            accent_color: Some("#123456".into()),
            ..Default::default()
        })
        .unwrap_err()
        .code,
        ErrorCode::Internal
    );
    assert!(lost.is_cancelled());
    assert_eq!(commits.load(Ordering::SeqCst), 1);
    assert_eq!(host.global_settings().unwrap(), before);
    assert_eq!(host.snapshot().unwrap(), snapshot);
    assert!(host.changed_since(snapshot.revision).unwrap().is_none());
    drop(host);
    let reopened = Host::open(&scratch.0).unwrap();
    assert_eq!(reopened.global_settings().unwrap(), before);
    assert_eq!(reopened.snapshot().unwrap(), snapshot);
}
#[test]
fn loss_at_normalization_return_is_rechecked_before_commit() {
    rejects_lost_commit(LoseAt::NormalizationReturn);
}
#[test]
fn loss_at_final_guard_rolls_back_all_sql_writes_revision_and_notification() {
    rejects_lost_commit(LoseAt::Commit);
}
