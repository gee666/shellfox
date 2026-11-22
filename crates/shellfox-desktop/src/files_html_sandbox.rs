use percent_encoding::percent_decode_str;
use std::{borrow::Cow, cell::Cell};
use wry::http;

pub(super) const SCHEME: &str = "shellfox-preview";
const SHELL: &str = "shellfox-preview://document/";
// Assets resolve relative to the document. "content" has no asset extension,
// so the document URL can never shadow a file the backend would serve.
const ASSETS: &str = "shellfox-preview://document/files/";
const CONTENT: &str = "shellfox-preview://document/files/content";
#[cfg(windows)]
pub(super) const ENTRY: &str = "http://shellfox-preview.document/";
#[cfg(not(windows))]
pub(super) const ENTRY: &str = SHELL;
#[cfg(windows)]
pub(super) const FRAME: &str = "http://shellfox-preview.document/files/content";
#[cfg(not(windows))]
const FRAME: &str = CONTENT;
#[cfg(windows)]
const ASSET_SOURCE: &str = "http://shellfox-preview.document/files/";
#[cfg(not(windows))]
const ASSET_SOURCE: &str = ASSETS;
const MAX_ASSET_PATH: usize = 4096;

// The outer document restricts frame navigation before the browser requests a
// URL. Wry's navigation callback is a second guard, not the network boundary.
const WRAPPER: &[u8] = br#"<!doctype html><meta charset="utf-8"><title>HTML preview</title>
<style>html,body,iframe{width:100%;height:100%;margin:0;border:0;overflow:hidden;background:white}iframe{display:block}</style>
<iframe title="HTML preview" sandbox="allow-scripts" referrerpolicy="no-referrer" src="/files/content"></iframe>"#;
// Keep ordinary link clicks on the preview instead of replacing it with the
// browser's blocked-URL page. This is UX; the outer CSP is the security check.
const KEEP_LINKS_IN_PREVIEW: &[u8] = br#"<script>(() => {
  const stop = (event) => {
    for (const node of event.composedPath()) {
      if (!(node instanceof Element) || !node.matches('a,area')) continue;
      const href = node.getAttribute('href') ?? node.getAttribute('xlink:href');
      if (href !== null && !href.trim().startsWith('#')) event.preventDefault();
      return;
    }
  };
  document.addEventListener('click', stop, true);
  document.addEventListener('auxclick', stop, true);
})();</script>"#;

fn content_policy() -> String {
    let assets = ASSET_SOURCE;
    format!(
        "default-src 'none'; script-src 'unsafe-inline' data: blob: {assets}; style-src 'unsafe-inline' data: blob: {assets}; img-src data: blob: {assets}; font-src data: blob: {assets}; media-src data: blob:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'"
    )
}

#[derive(Default)]
pub(super) struct Navigation {
    shell: Cell<bool>,
    frame: Cell<bool>,
}
impl Navigation {
    pub fn allow(&self, url: &str) -> bool {
        if url == ENTRY || url == SHELL {
            !self.shell.replace(true)
        } else if url == FRAME || url == CONTENT {
            self.shell.get() && !self.frame.replace(true)
        } else {
            false
        }
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum Route {
    Shell,
    Content,
    /// A document-relative path. The document backend enforces the allowed root.
    Asset(String),
    Denied,
}

pub(super) fn route(request: &http::Request<Vec<u8>>) -> Route {
    if request.method() != http::Method::GET {
        return Route::Denied;
    }
    let uri = request.uri().to_string();
    if uri == SHELL {
        return Route::Shell;
    }
    if uri == CONTENT {
        return Route::Content;
    }
    // The browser drops fragments and the query is never part of a file path.
    let Some(path) = uri.strip_prefix(ASSETS).map(|rest| rest.split('?').next().unwrap_or(""))
    else {
        return Route::Denied;
    };
    asset_path(path).map_or(Route::Denied, Route::Asset)
}

fn asset_path(encoded: &str) -> Option<String> {
    if encoded.is_empty() || encoded.len() > MAX_ASSET_PATH {
        return None;
    }
    let path = percent_decode_str(encoded).decode_utf8().ok()?;
    let clean = path
        .split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
        && !path.contains(['\\', ':', '\0']);
    clean.then(|| path.into_owned())
}

pub(super) fn page(route: &Route, document: &[u8]) -> http::Response<Cow<'static, [u8]>> {
    let (policy, body) = match route {
        Route::Shell => (
            format!(
                "default-src 'none'; style-src 'unsafe-inline'; frame-src {FRAME}; base-uri 'none'; form-action 'none'"
            ),
            Cow::Borrowed(WRAPPER),
        ),
        Route::Content => {
            let policy = content_policy();
            let mut page = format!("<!doctype html><meta http-equiv=\"Content-Security-Policy\" content=\"{policy}\"><meta name=\"referrer\" content=\"no-referrer\">").into_bytes();
            page.extend_from_slice(KEEP_LINKS_IN_PREVIEW);
            page.extend_from_slice(document);
            (format!("{policy}; sandbox allow-scripts"), Cow::Owned(page))
        }
        Route::Asset(_) | Route::Denied => return not_found(),
    };
    headers("text/html; charset=utf-8")
        .header(http::header::CONTENT_SECURITY_POLICY, policy)
        .body(body)
        .expect("static response headers")
}

pub(super) fn asset(mime_type: &str, bytes: Vec<u8>) -> http::Response<Cow<'static, [u8]>> {
    headers(mime_type)
        // The sandboxed document has an opaque origin; fonts and module scripts need CORS.
        .header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(http::header::CONTENT_SECURITY_POLICY, "default-src 'none'; sandbox")
        .body(Cow::Owned(bytes))
        .expect("asset response headers")
}

pub(super) fn not_found() -> http::Response<Cow<'static, [u8]>> {
    http::Response::builder()
        .status(404)
        .header(http::header::CACHE_CONTROL, "no-store")
        .body(Cow::Borrowed(&[][..]))
        .expect("static response")
}

fn headers(mime_type: &str) -> http::response::Builder {
    http::Response::builder()
        .header(http::header::CONTENT_TYPE, mime_type)
        .header(http::header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(http::header::REFERRER_POLICY, "no-referrer")
        .header(http::header::CACHE_CONTROL, "no-store")
        .header("X-DNS-Prefetch-Control", "off")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, url: &str) -> Route {
        let request = http::Request::builder()
            .uri(url)
            .method(method)
            .body(Vec::new())
            .unwrap();
        route(&request)
    }

    #[test]
    fn separates_the_trusted_frame_from_the_opaque_document() {
        let shell = page(&request("GET", SHELL), b"<h1>preview</h1>");
        assert_eq!(shell.status(), 200);
        assert!(
            shell.headers()[http::header::CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap()
                .contains(&format!("frame-src {FRAME};"))
        );
        let wrapper = std::str::from_utf8(shell.body()).unwrap();
        assert!(wrapper.contains("sandbox=\"allow-scripts\""));
        assert!(!wrapper.contains("<h1>preview</h1>"));
        let document = page(&request("GET", CONTENT), b"<h1>preview</h1>");
        assert_eq!(document.status(), 200);
        assert_eq!(
            document.headers()[http::header::CONTENT_SECURITY_POLICY],
            format!("{}; sandbox allow-scripts", content_policy())
        );
        assert!(content_policy().contains(&format!("img-src data: blob: {ASSET_SOURCE};")));
        assert!(content_policy().contains("connect-src 'none'"));
        assert!(document.body().ends_with(b"<h1>preview</h1>"));
    }

    #[test]
    fn maps_only_clean_document_relative_paths_to_assets() {
        assert_eq!(
            request("GET", "shellfox-preview://document/files/css/site.css?v=2"),
            Route::Asset("css/site.css".into())
        );
        assert_eq!(
            request("GET", "shellfox-preview://document/files/logo%20small.png"),
            Route::Asset("logo small.png".into())
        );
        for (method, url) in [
            ("POST", CONTENT),
            ("POST", "shellfox-preview://document/files/a.png"),
            ("GET", "shellfox-preview://document/file"),
            ("GET", "shellfox-preview://document/files/"),
            ("GET", "shellfox-preview://document/files/%2E%2E/secret.css"),
            ("GET", "shellfox-preview://document/files/a/%2e/b.css"),
            ("GET", "shellfox-preview://document/files/a//b.css"),
            ("GET", "shellfox-preview://document/files/..%5Csecret.css"),
            ("GET", "shellfox-preview://document/files/C%3A/secret.css"),
            ("GET", "shellfox-preview://document/files/a%00.css"),
            ("GET", "shellfox-preview://wrong/"),
            ("GET", "shellfox-preview://document/?extra=1"),
            ("GET", "file://local/content"),
        ] {
            assert_eq!(request(method, url), Route::Denied, "{method} {url}");
        }
        assert_eq!(page(&Route::Denied, b"x").status(), 404);
        assert!(page(&Route::Denied, b"x").body().is_empty());
    }

    #[test]
    fn navigates_only_to_the_shell_then_the_document_once() {
        let nav = Navigation::default();
        assert!(!nav.allow(FRAME));
        assert!(nav.allow(ENTRY));
        assert!(nav.allow(FRAME));
        assert!(!nav.allow(ENTRY));
        assert!(!nav.allow(FRAME));
        assert!(!nav.allow("http://127.0.0.1:58091/navigation"));
    }
}
