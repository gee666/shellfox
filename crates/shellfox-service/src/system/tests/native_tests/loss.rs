use super::*;

struct CancelAfterWrite {
    inner: Arc<NativeSystemFiles>,
    lost: CancellationToken,
    writes: AtomicUsize,
}
impl SystemFiles for CancelAfterWrite {
    fn read(&self, path: &Path) -> SystemResult<Option<SystemFile>> {
        self.inner.read(path)
    }
    fn directory(&self, path: &Path) -> SystemResult<bool> {
        self.inner.directory(path)
    }
    fn program(&self, path: &Path) -> SystemResult<bool> {
        self.inner.program(path)
    }
    fn canonical_directory(&self, path: &Path) -> SystemResult<std::path::PathBuf> {
        self.inner.canonical_directory(path)
    }
    fn replace(
        &self,
        path: &Path,
        expected: Option<&SystemFile>,
        bytes: Option<&[u8]>,
        executable: bool,
    ) -> SystemResult<()> {
        self.inner.replace(path, expected, bytes, executable)?;
        if self.writes.fetch_add(1, Ordering::SeqCst) == 0 {
            self.lost.cancel();
        }
        Ok(())
    }
}
#[test]
fn accepted_partial_external_write_is_not_retried_or_reported_as_success_after_loss() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    let authority = Authority::local(41);
    let lost = CancellationToken::new();
    let writes = Arc::new(CancelAfterWrite {
        inner: fixture.files.clone(),
        lost: lost.clone(),
        writes: AtomicUsize::new(0),
    });
    let mut backend = fixture.backend(
        Vec::new(),
        SystemInstallPolicy {
            allow_user_install: true,
            allow_open: false,
        },
    );
    Arc::get_mut(&mut backend).unwrap().files = writes.clone();
    let handler = SystemNativeHandler::new(backend, lost).unwrap();
    let error = handler
        .handle(
            41,
            SystemNativeRequest::SetCli {
                scope: SystemScope::User,
                installed: true,
            },
            &authority,
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NativeUnavailable);
    assert!(!error.retryable);
    assert_eq!(writes.writes.load(Ordering::SeqCst), 1);
    assert!(!fixture.plan.marker.exists());
    assert_eq!(fixture.registry.writes.load(Ordering::SeqCst), 0);
}
