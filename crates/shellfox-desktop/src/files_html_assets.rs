use super::{MAX_DOCUMENT, markup, relative_asset};
use crate::files::DocumentBackend;
use base64::{Engine, engine::general_purpose::STANDARD};
use regex::Regex;
use shellfox_files::EditorCancellation;
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, LazyLock},
};

const MAX_ASSETS: usize = 100;
const MAX_ASSET_CHARS: usize = 8 * 1024 * 1024;
static CSS_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)@import\s+(?:url\(\s*)?["']([^"']+)["']\s*\)?\s*([^;]*);"#)
        .expect("CSS import pattern")
});
static CSS_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)url\(\s*(?:"([^"]*)"|'([^']*)'|([^\s)]*))\s*\)"#).expect("CSS URL pattern")
});

pub(super) struct Prepared {
    pub bytes: Arc<[u8]>,
    pub warnings: Vec<String>,
}

struct Asset {
    mime_type: String,
    bytes: Vec<u8>,
}
impl Asset {
    fn data(&self) -> String {
        format!(
            "data:{};base64,{}",
            self.mime_type,
            STANDARD.encode(&self.bytes)
        )
    }
}

struct Assets<'a> {
    backend: &'a DocumentBackend,
    document: &'a str,
    cancel: EditorCancellation,
    cache: HashMap<String, Option<Asset>>,
    asset_chars: usize,
    warnings: Vec<String>,
}

pub(super) async fn prepare(
    backend: DocumentBackend,
    path: String,
    source: Arc<str>,
    cancel: EditorCancellation,
) -> anyhow::Result<Prepared> {
    let refs = markup::references(&source)?;
    let mut assets = Assets {
        backend: &backend,
        document: &path,
        cancel,
        cache: HashMap::new(),
        asset_chars: 0,
        warnings: Vec::new(),
    };
    let mut stylesheets = HashMap::new();
    for (path, css) in refs.assets {
        if css {
            if let Some(text) = assets.stylesheet(&path).await? {
                let css = assets.css(&text, directory(&path), 0).await?;
                let data = format!("data:text/css;base64,{}", STANDARD.encode(css));
                assets.reserve(data.len())?;
                stylesheets.insert(path, data);
            }
        } else {
            assets.load(&path).await?;
        }
    }
    let mut styles = HashMap::new();
    for style in refs.styles {
        if !styles.contains_key(&style) {
            let css = assets.css(&style, "", 0).await?;
            if css != style {
                assets.reserve(css.len())?;
                styles.insert(style, css);
            }
        }
    }
    backend.check_current()?;
    let bytes = markup::rewrite(
        &source,
        |path, css| {
            if css {
                stylesheets.get(path).cloned()
            } else {
                assets.data(path)
            }
        },
        |css| styles.get(css).cloned().unwrap_or_else(|| css.to_owned()),
        MAX_DOCUMENT,
    )?;
    Ok(Prepared {
        bytes: bytes.into(),
        warnings: assets.warnings,
    })
}

impl Assets<'_> {
    fn reserve(&mut self, chars: usize) -> anyhow::Result<()> {
        self.asset_chars += chars;
        anyhow::ensure!(
            self.asset_chars <= MAX_ASSET_CHARS,
            "Preview assets exceed the 8 MiB document limit."
        );
        Ok(())
    }

    async fn load(&mut self, path: &str) -> anyhow::Result<()> {
        if self.cache.contains_key(path) {
            return Ok(());
        }
        if self.cache.len() == MAX_ASSETS {
            if self
                .warnings
                .last()
                .is_none_or(|warning| warning != "Preview asset limit reached.")
            {
                self.warnings
                    .push("Preview asset limit reached.".to_owned());
            }
            return Ok(());
        }
        self.backend.check_current()?;
        let asset = match self
            .backend
            .read_asset(self.document, path.to_owned(), self.cancel.clone())
            .await
        {
            Ok(value) => {
                // MIME is a header, not document content. Only accept plain MIME tokens.
                anyhow::ensure!(
                    value
                        .mime_type
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"/+-._".contains(&c)),
                    "Invalid preview asset type."
                );
                self.reserve(13 + value.mime_type.len() + value.bytes.len().div_ceil(3) * 4)?;
                Some(Asset {
                    mime_type: value.mime_type,
                    bytes: value.bytes,
                })
            }
            Err(error) => {
                self.warnings.push(format!("{path}: {error}"));
                None
            }
        };
        self.cache.insert(path.to_owned(), asset);
        Ok(())
    }

    fn data(&self, path: &str) -> Option<String> {
        self.cache.get(path)?.as_ref().map(Asset::data)
    }

    async fn stylesheet(&mut self, path: &str) -> anyhow::Result<Option<String>> {
        self.load(path).await?;
        Ok(self
            .cache
            .get(path)
            .and_then(Option::as_ref)
            .and_then(|asset| std::str::from_utf8(&asset.bytes).ok().map(str::to_owned)))
    }

    fn css<'a>(
        &'a mut self,
        text: &'a str,
        base: &'a str,
        depth: usize,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            if depth > 5 {
                return Ok(String::new());
            }
            let mut output = String::new();
            let mut cursor = 0;
            for import in CSS_IMPORT.captures_iter(text) {
                let full = import.get(0).expect("complete import match");
                self.css_urls(&text[cursor..full.start()], base, &mut output)
                    .await?;
                if let Some(path) = relative_asset(&import[1], base) {
                    if let Some(stylesheet) = self.stylesheet(&path).await? {
                        let css = self.css(&stylesheet, directory(&path), depth + 1).await?;
                        let media = import[2].trim();
                        if media.is_empty() {
                            push(&mut output, &css)?;
                        } else {
                            push(&mut output, &format!("@media {media}{{{css}}}"))?;
                        }
                    }
                }
                cursor = full.end();
            }
            self.css_urls(&text[cursor..], base, &mut output).await?;
            Ok(output)
        })
    }

    async fn css_urls(
        &mut self,
        text: &str,
        base: &str,
        output: &mut String,
    ) -> anyhow::Result<()> {
        let mut cursor = 0;
        for url in CSS_URL.captures_iter(text) {
            let full = url.get(0).expect("complete URL match");
            let value = (1..=3)
                .find_map(|group| url.get(group))
                .map_or("", |m| m.as_str());
            let Some(path) = relative_asset(value, base) else {
                continue;
            };
            push(output, &text[cursor..full.start()])?;
            self.load(&path).await?;
            push(
                output,
                &format!("url(\"{}\")", self.data(&path).unwrap_or_default()),
            )?;
            cursor = full.end();
        }
        push(output, &text[cursor..])
    }
}

fn directory(path: &str) -> &str {
    path.rfind('/').map_or("", |index| &path[..=index])
}

fn push(output: &mut String, text: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        output.len() + text.len() <= MAX_DOCUMENT,
        "Preview assets exceed the document size limit."
    );
    output.push_str(text);
    Ok(())
}
