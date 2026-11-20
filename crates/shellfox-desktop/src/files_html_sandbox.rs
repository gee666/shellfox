use std::{borrow::Cow, cell::Cell};
use wry::http;

pub(super) const SCHEME: &str = "shellfox-preview";
const SHELL: &str = "shellfox-preview://document/";
const CONTENT: &str = "shellfox-preview://document/content";
#[cfg(windows)]
pub(super) const ENTRY: &str = "http://shellfox-preview.document/";
#[cfg(not(windows))]
pub(super) const ENTRY: &str = SHELL;
#[cfg(windows)]
pub(super) const FRAME: &str = "http://shellfox-preview.document/content";
#[cfg(not(windows))]
const FRAME: &str = CONTENT;

// The outer document restricts frame navigation before the browser requests a
// URL. Wry's navigation callback is a second guard, not the network boundary.
const WRAPPER: &[u8] = br#"<!doctype html><meta charset="utf-8"><title>HTML preview</title>
<style>html,body,iframe{width:100%;height:100%;margin:0;border:0;overflow:hidden;background:white}iframe{display:block}</style>
<iframe title="HTML preview" sandbox="allow-scripts" referrerpolicy="no-referrer" src="/content"></iframe>"#;
const CONTENT_POLICY: &str = "default-src 'none'; script-src 'unsafe-inline' data: blob:; style-src 'unsafe-inline' data: blob:; img-src data: blob:; font-src data: blob:; media-src data: blob:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";
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

pub(super) fn respond(
    request: &http::Request<Vec<u8>>,
    bytes: &[u8],
) -> http::Response<Cow<'static, [u8]>> {
    let (policy, body) = if request.method() == http::Method::GET && request.uri() == SHELL {
        (
            format!(
                "default-src 'none'; style-src 'unsafe-inline'; frame-src {FRAME}; base-uri 'none'; form-action 'none'"
            ),
            Cow::Borrowed(WRAPPER),
        )
    } else if request.method() == http::Method::GET && request.uri() == CONTENT {
        let mut document = format!("<!doctype html><meta http-equiv=\"Content-Security-Policy\" content=\"{CONTENT_POLICY}\"><meta name=\"referrer\" content=\"no-referrer\">").into_bytes();
        document.extend_from_slice(KEEP_LINKS_IN_PREVIEW);
        document.extend_from_slice(bytes);
        (
            format!("{CONTENT_POLICY}; sandbox allow-scripts"),
            Cow::Owned(document),
        )
    } else {
        return http::Response::builder()
            .status(404)
            .body(Cow::Borrowed(&[][..]))
            .expect("static response");
    };
    http::Response::builder()
        .header(http::header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(http::header::CONTENT_SECURITY_POLICY, policy)
        .header(http::header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(http::header::REFERRER_POLICY, "no-referrer")
        .header(http::header::CACHE_CONTROL, "no-store")
        .header("X-DNS-Prefetch-Control", "off")
        .body(body)
        .expect("static response headers")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, url: &str) -> http::Response<Cow<'static, [u8]>> {
        let request = http::Request::builder()
            .uri(url)
            .method(method)
            .body(Vec::new())
            .unwrap();
        respond(&request, b"<h1>preview</h1>")
    }

    #[test]
    fn separates_the_trusted_frame_from_the_opaque_document() {
        let shell = request("GET", SHELL);
        assert_eq!(shell.status(), 200);
        assert!(
            shell.headers()[http::header::CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap()
                .contains(&format!("frame-src {FRAME}"))
        );
        assert!(
            std::str::from_utf8(shell.body())
                .unwrap()
                .contains("sandbox=\"allow-scripts\"")
        );
        assert!(
            !std::str::from_utf8(shell.body())
                .unwrap()
                .contains("<h1>preview</h1>")
        );
        let document = request("GET", CONTENT);
        assert_eq!(document.status(), 200);
        assert_eq!(
            document.headers()[http::header::CONTENT_SECURITY_POLICY],
            format!("{CONTENT_POLICY}; sandbox allow-scripts")
        );
        assert!(document.body().ends_with(b"<h1>preview</h1>"));
    }

    #[test]
    fn never_serves_external_requests_or_uses_a_url_as_a_file_path() {
        for (method, url) in [
            ("POST", CONTENT),
            ("GET", "shellfox-preview://document/file"),
            ("GET", "shellfox-preview://wrong/"),
            ("GET", "shellfox-preview://document/content?extra=1"),
            ("GET", "file://local/content"),
        ] {
            let denied = request(method, url);
            assert_eq!(denied.status(), 404);
            assert!(denied.body().is_empty());
        }
        let nav = Navigation::default();
        assert!(!nav.allow(FRAME));
        assert!(nav.allow(ENTRY));
        assert!(nav.allow(FRAME));
        assert!(!nav.allow(ENTRY));
        assert!(!nav.allow(FRAME));
        assert!(!nav.allow("http://127.0.0.1:58091/navigation"));
    }
}
