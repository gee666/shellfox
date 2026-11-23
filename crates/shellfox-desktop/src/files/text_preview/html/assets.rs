//! Serves a previewed document's local assets to the webview on request.
use crate::files::DocumentBackend;
use gpui::BackgroundExecutor;
use shellfox_files::EditorCancellation;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

const MAX_ASSETS: usize = 100;
const MAX_ASSET_BYTES: usize = 8 * 1024 * 1024;
const LIMIT_WARNING: &str = "Preview asset limit reached.";

pub(super) struct Page {
    pub document: Arc<[u8]>,
    backend: DocumentBackend,
    path: String,
    cancel: EditorCancellation,
    executor: BackgroundExecutor,
    loaded: Mutex<Loaded>,
    reads: tokio::sync::Semaphore,
    warnings: watch::Sender<Vec<String>>,
}

#[derive(Default)]
struct Loaded {
    assets: HashMap<String, Option<Arc<Asset>>>,
    /// Misses being read now; they count against MAX_ASSETS.
    pending: usize,
    bytes: usize,
}

struct Asset {
    mime_type: String,
    bytes: Vec<u8>,
}

pub(super) type Response = wry::http::Response<std::borrow::Cow<'static, [u8]>>;

impl Page {
    pub fn new(
        document: Arc<[u8]>,
        backend: DocumentBackend,
        path: String,
        cancel: EditorCancellation,
        executor: BackgroundExecutor,
    ) -> Self {
        Self {
            document,
            backend,
            path,
            cancel,
            executor,
            loaded: Mutex::default(),
            // Matches the host's asset-read budget for this session.
            reads: tokio::sync::Semaphore::new(shellfox_files::ASSET_READS_PER_ACTOR),
            warnings: watch::Sender::new(Vec::new()),
        }
    }

    pub fn warnings(&self) -> Vec<String> {
        self.warnings.borrow().clone()
    }

    pub fn watch_warnings(&self) -> watch::Receiver<Vec<String>> {
        self.warnings.subscribe()
    }

    /// Resolves on a background thread; wry hands the response back to the webview's thread.
    pub fn serve(self: &Arc<Self>, path: String, respond: impl FnOnce(Response) + Send + 'static) {
        let page = self.clone();
        self.executor
            .spawn(async move { respond(page.load(path).await) })
            .detach();
    }

    async fn load(&self, path: String) -> Response {
        let cached = {
            let mut loaded = self.loaded.lock().expect("preview assets");
            match loaded.assets.get(&path) {
                Some(asset) => Some(asset.clone()),
                None if loaded.assets.len() + loaded.pending >= MAX_ASSETS => {
                    drop(loaded);
                    self.warn_once(LIMIT_WARNING);
                    return super::sandbox::not_found();
                }
                None => {
                    loaded.pending += 1;
                    None
                }
            }
        };
        let asset = match cached {
            Some(asset) => asset,
            None => self.read(&path).await,
        };
        match asset {
            Some(asset) => super::sandbox::asset(&asset.mime_type, asset.bytes.clone()),
            None => super::sandbox::not_found(),
        }
    }

    async fn read(&self, path: &str) -> Option<Arc<Asset>> {
        let _reading = self.reads.acquire().await.expect("the read budget is never closed");
        let result = match self.backend.check_current() {
            Ok(()) => {
                self.backend
                    .read_asset(&self.path, path.to_owned(), self.cancel.clone())
                    .await
            }
            Err(error) => Err(error),
        };
        let asset = match result {
            // MIME is a header, not document content. Only accept plain MIME tokens.
            Ok(value)
                if !value.mime_type.is_empty()
                    && value
                        .mime_type
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"/+-._".contains(&c)) =>
            {
                Ok(Asset {
                    mime_type: value.mime_type,
                    bytes: value.bytes,
                })
            }
            Ok(_) => Err("Invalid preview asset type.".to_owned()),
            Err(error) => Err(error.to_string()),
        };
        let mut loaded = self.loaded.lock().expect("preview assets");
        loaded.pending -= 1;
        // Another request for the same path may have finished first.
        if let Some(existing) = loaded.assets.get(path) {
            return existing.clone();
        }
        let asset = match asset {
            Ok(asset) if loaded.bytes + asset.bytes.len() <= MAX_ASSET_BYTES => {
                loaded.bytes += asset.bytes.len();
                Some(Arc::new(asset))
            }
            Ok(_) => {
                self.warn_once(LIMIT_WARNING);
                None
            }
            Err(error) => {
                self.warnings
                    .send_modify(|warnings| warnings.push(format!("{path}: {error}")));
                None
            }
        };
        loaded.assets.insert(path.to_owned(), asset.clone());
        asset
    }

    /// A reference above the document's folder: never read, listed once.
    pub fn outside(&self, path: &str) {
        self.warn_once(&format!("{path}: Preview asset is outside the HTML directory."));
    }

    fn warn_once(&self, warning: &str) {
        self.warnings.send_if_modified(|warnings| {
            let new = !warnings.iter().any(|old| old == warning);
            if new {
                warnings.push(warning.to_owned());
            }
            new
        });
    }
}
