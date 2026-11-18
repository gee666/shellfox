use super::*;
use crate::{activity_integration::*, remote::SourceFactsProjector, system::*};
use shellfox_core::{Preferences, Selection, Settings, Snapshot};
use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
    thread::ThreadId,
};
struct Queries {
    home: PathBuf,
    caller: ThreadId,
    calls: AtomicUsize,
    fail: bool,
}
impl Queries {
    fn queried(&self) {
        assert_ne!(
            std::thread::current().id(),
            self.caller,
            "queries must run off owner"
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}
fn missing() -> wire::AppError {
    wire::AppError {
        code: wire::ErrorCode::NativeUnavailable,
        message: "Owned fixture query unavailable.".into(),
        retryable: false,
    }
}
impl SystemBackend for Queries {
    fn home_directory(&self) -> SystemResult<wire::Directory> {
        self.queried();
        if self.fail {
            return Err(missing());
        }
        Ok(wire::Directory {
            cwd: self.home.to_str().unwrap().into(),
        })
    }
    fn cli_integration(&self, scope: SystemScope) -> SystemResult<wire::CliIntegration> {
        self.queried();
        assert_eq!(scope, SystemScope::User);
        if self.fail {
            return Err(missing());
        }
        Ok(wire::CliIntegration {
            supported: false,
            installed: false,
            command: "shellfox".into(),
            reason: Some("Owned fixture has no install launch contract.".into()),
        })
    }
    fn explorer_integration(&self, scope: SystemScope) -> SystemResult<wire::ExplorerIntegration> {
        self.queried();
        assert_eq!(scope, SystemScope::User);
        Ok(wire::ExplorerIntegration {
            supported: false,
            installed: false,
            folder_item_installed: false,
            background_installed: false,
            reason: Some("Owned fixture has no Explorer installation adapter.".into()),
        })
    }
    fn perform_user_action(&self, _: SystemAction) -> SystemResult<SystemResponse> {
        panic!("facts must never invoke an installation/open action")
    }
}
fn source() -> Arc<SourceFacts> {
    let (mut publisher, watch) = SourceFactsPublisher::new();
    publisher
        .publish(
            Snapshot {
                revision: 1,
                sessions: vec![],
                preferences: Preferences::default(),
                selection: Selection::default(),
            },
            Settings::default(),
            vec![],
            wire::CancellationToken::new(),
        )
        .unwrap();
    let source = read_source(&watch).unwrap();
    // Source lifetime is tested through the slot separately; this immutable value
    // has the real capture and cancellation fields and contains no Runtime/Host.
    source
}
fn request(source: &SourceFacts) -> ManagerFactsRequest {
    ManagerFactsRequest {
        target_session: None,
        private_tabs: Default::default(),
        snapshot: source.snapshot.clone(),
        settings: source.settings.clone(),
        terminals: vec![],
        cancelled: source.cancelled.child_token(),
    }
}
fn fixtures(fail: bool) -> (tempfile::TempDir, NativeSourceFactsProjector, Arc<Queries>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let scratch = tempfile::Builder::new()
        .prefix("facts-projector-")
        .tempdir_in(root)
        .unwrap();
    let resource = scratch.path().join("readonly-supervisor-resource");
    std::fs::write(&resource, b"not an executable; readonly evidence only").unwrap();
    let queries = Arc::new(Queries {
        home: scratch.path().into(),
        caller: std::thread::current().id(),
        calls: AtomicUsize::new(0),
        fail,
    });
    let system = Arc::new(HostSystemServices::new(queries.clone()));
    let projector = NativeSourceFactsProjector::new(resource, system, SystemScope::User).unwrap();
    (scratch, projector, queries)
}
#[tokio::test(flavor = "current_thread")]
async fn empty_host_has_actual_probe_and_read_only_system_queries_without_invented_processes() {
    let (_scratch, projector, queries) = fixtures(false);
    let source = source();
    let result = projector
        .project(source.clone(), request(&source))
        .await
        .unwrap();
    let probe = result
        .facts
        .probe
        .as_ref()
        .expect("empty host still has backend capability evidence");
    assert!(result.facts.sessions.is_empty());
    assert_eq!(
        probe.platform,
        match std::env::consts::OS {
            "windows" => "win32",
            "macos" => "darwin",
            os => os,
        }
    );
    assert_eq!(
        probe.available,
        result
            .profiles
            .as_ref()
            .unwrap()
            .profiles
            .iter()
            .any(|profile| profile.available)
    );
    assert_eq!(queries.calls.load(Ordering::SeqCst), 3);
    assert!(result.home.is_some());
    assert!(!result.facts.cli.as_ref().unwrap().installed);
    shellfox_remote::adapter::manager_snapshot_with_settings(
        &source.snapshot,
        &result.facts,
        &source.settings,
    )
    .unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn capability_collection_does_not_invoke_the_strict_v1_unknown_creation_mapper() {
    let (_scratch, projector, queries) = fixtures(false);
    let mut input = (*source()).clone();
    input.snapshot.sessions.push(serde_json::from_value(serde_json::json!({
        "id":"11111111-1111-4111-8111-111111111111", "title":"raw metadata", "cwd":"/owned",
        "createdAt":"2026-01-01T00:00:00.000Z", "updatedAt":"2026-01-01T00:00:00.000Z",
        "settledAt":null,"pinnedAt":null,"env":[],"tabs":[{
            "id":"22222222-2222-4222-8222-222222222222", "sessionId":"11111111-1111-4111-8111-111111111111",
            "title":"raw", "cwd":"/owned", "ordinal":0,"profileId":"automation-pipe","state":"pending"
        }]
    })).unwrap());
    let source = Arc::new(input);
    assert!(projector
        .project(source.clone(), request(&source))
        .await
        .is_err());
    let capabilities = projector
        .project_capabilities(source.clone(), request(&source))
        .await
        .unwrap();
    assert!(capabilities.facts.sessions.is_empty());
    assert!(capabilities.profiles.is_some());
    assert!(capabilities.home.is_some());
    assert!(capabilities.facts.probe.is_some());
    assert_eq!(queries.calls.load(Ordering::SeqCst), 6);
    assert!(
        source.creation_facts.is_empty(),
        "native capabilities never backfill a raw birth"
    );
}
#[tokio::test(flavor = "current_thread")]
async fn independent_provider_errors_remain_missing_instead_of_fabricated_installation_or_home() {
    let (_scratch, projector, queries) = fixtures(true);
    let source = source();
    let result = projector
        .project(source.clone(), request(&source))
        .await
        .unwrap();
    assert!(result.home.is_none());
    assert!(result.facts.cli.is_none());
    assert!(result.facts.explorer.is_some());
    assert_eq!(queries.calls.load(Ordering::SeqCst), 3);
    assert!(shellfox_remote::adapter::manager_snapshot_with_settings(
        &source.snapshot,
        &result.facts,
        &source.settings
    )
    .is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn cancellation_and_mismatched_owner_request_start_no_discovery_queries() {
    let (_scratch, projector, queries) = fixtures(false);
    let source = source();
    let mut changed = request(&source);
    changed.snapshot.revision += 1;
    assert!(projector.project(source.clone(), changed).await.is_err());
    source.cancelled.cancel();
    assert!(projector
        .project(source.clone(), request(&source))
        .await
        .is_err());
    assert_eq!(queries.calls.load(Ordering::SeqCst), 0);
}
